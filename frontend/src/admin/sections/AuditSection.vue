<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { ChevronDown, ChevronRight, RefreshCw, Search } from 'lucide-vue-next'
import * as api from '@/api/endpoints'
import type { AuditEvent } from '@/api/types'
import { t } from '@/i18n'
import { errorMessage, fmtDateTime, fmtRelative } from '@/lib/format'
import { actionTone, hasDetails, KNOWN_ACTIONS, mergeEvents, nextCursor } from '../lib/audit'
import { pretty } from '../lib/json'
import StateBlock from '../components/StateBlock.vue'

const PAGE = 100
const events = ref<AuditEvent[]>([])
const loading = ref(false)
const loadingMore = ref(false)
const error = ref<string | null>(null)
const moreError = ref<string | null>(null)
const hasMore = ref(false)
const action = ref('')
const actor = ref('')
const open = ref<Set<number>>(new Set())
let seq = 0
let timer: ReturnType<typeof setTimeout> | undefined

const params = computed(() => ({ limit: PAGE, action: action.value.trim() || undefined, actor: actor.value.trim() || undefined }))

async function load() {
  const my = ++seq
  loading.value = true
  try {
    const r = await api.admin.audit(params.value)
    if (my !== seq) return
    events.value = r.events
    hasMore.value = r.events.length >= PAGE
    error.value = null
    open.value = new Set()
  } catch (e) {
    if (my !== seq) return
    error.value = errorMessage(e)
  } finally {
    if (my === seq) loading.value = false
  }
}

async function older() {
  const before = nextCursor(events.value)
  if (before === undefined) return
  loadingMore.value = true
  moreError.value = null
  try {
    const r = await api.admin.audit({ ...params.value, before })
    events.value = mergeEvents(events.value, r.events)
    hasMore.value = r.events.length >= PAGE
  } catch (e) {
    moreError.value = errorMessage(e)
  } finally {
    loadingMore.value = false
  }
}

function toggle(id: number) {
  const n = new Set(open.value)
  if (n.has(id)) n.delete(id)
  else n.add(id)
  open.value = n
}

watch([action, actor], () => {
  clearTimeout(timer)
  timer = setTimeout(load, 300)
})
onMounted(load)
onBeforeUnmount(() => clearTimeout(timer))
</script>

<template>
  <section class="adm-page">
    <header class="adm-head">
      <div class="grow">
        <h1>{{ t('admin.audit.title') }}</h1>
        <p class="muted">{{ t('admin.audit.subtitle') }}</p>
      </div>
      <button class="btn btn-icon" :title="t('common.refresh')" :aria-label="t('common.refresh')" :disabled="loading" @click="load"><RefreshCw :size="15" /></button>
    </header>

    <div class="filters audit-filters">
      <div class="field">
        <label for="audit-action">{{ t('admin.audit.filterAction') }}</label>
        <input id="audit-action" v-model="action" class="input" list="audit-actions" :placeholder="t('admin.audit.actionPlaceholder')" autocomplete="off" spellcheck="false" />
        <datalist id="audit-actions"><option v-for="a in KNOWN_ACTIONS" :key="a" :value="a" /></datalist>
      </div>
      <div class="field">
        <label for="audit-actor">{{ t('admin.audit.filterActor') }}</label>
        <div class="adm-search"><Search :size="15" aria-hidden="true" /><input id="audit-actor" v-model="actor" class="input" type="search" :placeholder="t('admin.audit.actorPlaceholder')" autocomplete="off" /></div>
      </div>
    </div>

    <StateBlock :loading="loading && !events.length" :error="error" :empty="!events.length" :empty-text="t('admin.audit.empty')" @retry="load">
      <div class="card tablewrap">
        <table class="table audit-table">
          <thead>
            <tr><th class="chev-col" /><th>{{ t('admin.audit.colTime') }}</th><th>{{ t('admin.audit.colActor') }}</th><th>{{ t('admin.audit.colAction') }}</th><th>{{ t('admin.audit.colTarget') }}</th><th>{{ t('admin.audit.colIp') }}</th></tr>
          </thead>
          <tbody>
            <template v-for="e in events" :key="e.id">
              <tr :class="{ open: open.has(e.id) }">
                <td class="chev-col">
                  <button v-if="hasDetails(e)" class="btn btn-sm btn-icon btn-ghost" :aria-expanded="open.has(e.id)" :aria-label="t('admin.audit.details')" @click="toggle(e.id)">
                    <ChevronDown v-if="open.has(e.id)" :size="15" /><ChevronRight v-else :size="15" />
                  </button>
                </td>
                <td class="nowrap" :title="e.at">{{ fmtDateTime(e.at) }}<div class="faint">{{ fmtRelative(e.at) }}</div></td>
                <td>{{ e.actor || '—' }}</td>
                <td><span class="tag tag-mono" :class="`tag-${actionTone(e.action)}`">{{ e.action }}</span></td>
                <td class="mono target"><span class="faint tt">{{ e.targetType }}</span><span v-if="e.targetId" class="truncate" :title="e.targetId">{{ e.targetId }}</span></td>
                <td class="mono faint">{{ e.ip || '—' }}</td>
              </tr>
              <tr v-if="open.has(e.id)" class="detail-row"><td /><td colspan="5"><pre class="json-pre">{{ pretty(e.details) }}</pre></td></tr>
            </template>
          </tbody>
        </table>
      </div>
      <div class="row more">
        <span class="faint">{{ t('admin.audit.shown', { n: events.length }) }}</span>
        <span class="spacer" />
        <span v-if="moreError" class="banner banner-err" role="alert">{{ moreError }}</span>
        <button v-if="hasMore" class="btn" :disabled="loadingMore" @click="older"><span v-if="loadingMore" class="spinner" />{{ t('admin.audit.older') }}</button>
        <span v-else class="faint">{{ t('admin.audit.end') }}</span>
      </div>
    </StateBlock>
  </section>
</template>
