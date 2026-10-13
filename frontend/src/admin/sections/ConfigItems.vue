<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { ArrowUpCircle, Pencil, Plus, RefreshCw, RotateCcw, Search, Trash2 } from 'lucide-vue-next'
import * as api from '@/api/endpoints'
import { ApiError } from '@/api/client'
import type { ConfigItem } from '@/api/types'
import { has, lt, t } from '@/i18n'
import { errorMessage, fmtRelative } from '@/lib/format'
import { useDialogs } from '@/stores/dialogs'
import { useToasts } from '@/stores/toasts'
import { itemMatches, itemTitle, kindMeta } from '../lib/configKinds'
import { useAdminConfig } from '../store'
import ConfigItemEditor from '../components/ConfigItemEditor.vue'
import StateBlock from '../components/StateBlock.vue'

const props = defineProps<{ kind: string }>()
defineSlots<{
  /** content between the page header and the list (a status card) */
  top?(): unknown
  /** extra header cells (before the status column) */
  head?(): unknown
  /** extra cells of a row; `patch` saves a modified copy of the item's data */
  default?(p: { item: ConfigItem; patch: (item: ConfigItem, data: unknown) => Promise<boolean>; busy: boolean }): unknown
}>()

const route = useRoute()
const router = useRouter()
const toasts = useToasts()
const dialogs = useDialogs()
const store = useAdminConfig()

const items = ref<ConfigItem[]>([])
const loading = ref(false)
const error = ref<string | null>(null)
const query = ref('')
const filter = ref<'all' | 'modified' | 'custom' | 'updates' | 'errors'>('all')
const editing = ref<{ item: ConfigItem | null } | null>(null)
const busy = ref<Set<string>>(new Set())
let seq = 0

const meta = computed(() => kindMeta(props.kind))
const titleKey = computed(() => `admin.kind.${props.kind}.title`)
const errorIds = computed(() => new Map(store.errors.filter((e) => e.kind === props.kind).map((e) => [e.id, e.error])))

const rows = computed(() => items.value.map((item) => ({ item, ...itemTitle(props.kind, item.id, item.data, lt) })))
const filtered = computed(() => rows.value.filter((r) => {
  const i = r.item
  if (filter.value === 'modified' && !i.modified) return false
  if (filter.value === 'custom' && i.origin !== 'custom') return false
  if (filter.value === 'updates' && !i.updateAvailable) return false
  if (filter.value === 'errors' && !errorIds.value.has(i.id)) return false
  return itemMatches(i.id, r.title, i.data, query.value)
}))
const counts = computed(() => ({
  modified: items.value.filter((i) => i.modified).length,
  custom: items.value.filter((i) => i.origin === 'custom').length,
  updates: items.value.filter((i) => i.updateAvailable).length,
  errors: errorIds.value.size,
}))

async function load(showSpinner = true) {
  const my = ++seq
  if (showSpinner) loading.value = true
  try {
    const r = await api.admin.configList(props.kind)
    if (my !== seq) return
    items.value = r.items
    error.value = null
    openFromRoute()
  } catch (e) {
    if (my !== seq) return
    error.value = errorMessage(e)
  } finally {
    if (my === seq) loading.value = false
  }
}

/** `?item=<id>` opens the editor of that item (deep link from the parse-error banner). */
function openFromRoute() {
  const id = route.query.item
  if (typeof id !== 'string' || editing.value) return
  const found = items.value.find((i) => i.id === id)
  if (found) editing.value = { item: found }
}

function closeEditor() {
  editing.value = null
  if (route.query.item) router.replace({ path: route.path, query: { ...route.query, item: undefined } })
}

async function afterChange() {
  await Promise.all([load(false), store.load()])
}

async function onSaved() {
  closeEditor()
  await afterChange()
}

async function remove(item: ConfigItem) {
  const ok = await dialogs.confirm({ title: t('admin.items.deleteTitle', { id: item.id }), text: t('admin.items.deleteText'), confirmLabel: t('common.delete'), danger: true })
  if (!ok) return
  await run(item.id, async () => {
    await api.admin.configDelete(props.kind, item.id)
    toasts.ok(t('admin.items.deleted', { id: item.id }))
  })
}

async function reset(item: ConfigItem) {
  const upd = item.updateAvailable
  const ok = await dialogs.confirm({
    title: upd ? t('admin.items.resetNewTitle', { id: item.id }) : t('admin.items.resetTitle', { id: item.id }),
    text: upd ? t('admin.items.resetNewText') : t('admin.items.resetText'),
    confirmLabel: upd ? t('admin.items.resetNew') : t('admin.items.reset'),
    danger: !upd,
  })
  if (!ok) return
  const done = await run(item.id, async () => {
    await api.admin.configReset(props.kind, item.id)
    toasts.ok(t('admin.items.resetDone', { id: item.id }))
  })
  if (done && editing.value?.item?.id === item.id) closeEditor()
}

/** Save a modified copy of `item.data` (inline controls of the rules table). */
async function patch(item: ConfigItem, data: unknown) {
  return run(item.id, async () => {
    await api.admin.configPut(props.kind, item.id, data)
    toasts.ok(t('admin.items.saved', { id: item.id }))
  })
}

async function run(id: string, fn: () => Promise<void>): Promise<boolean> {
  busy.value = new Set(busy.value).add(id)
  try {
    await fn()
    await afterChange()
    return true
  } catch (e) {
    toasts.error(e instanceof ApiError ? e.message : errorMessage(e))
    return false
  } finally {
    const next = new Set(busy.value)
    next.delete(id)
    busy.value = next
  }
}

onMounted(() => load())
watch(() => props.kind, () => { items.value = []; query.value = ''; filter.value = 'all'; editing.value = null; load() })
watch(() => route.query.item, () => openFromRoute())
defineExpose({ reload: (spinner = true) => load(spinner) })
</script>

<template>
  <section class="adm-page">
    <header class="adm-head">
      <div class="grow">
        <h1>{{ has(titleKey) ? t(titleKey) : kind }}</h1>
        <p class="muted">{{ t(`admin.kind.${kind}.desc`) }}</p>
      </div>
      <div class="adm-search">
        <Search :size="15" aria-hidden="true" />
        <input v-model="query" class="input" type="search" :placeholder="t('admin.items.search')" :aria-label="t('admin.items.search')" />
      </div>
      <button class="btn btn-icon" :title="t('common.refresh')" :aria-label="t('common.refresh')" :disabled="loading" @click="afterChange"><RefreshCw :size="15" /></button>
      <button v-if="!meta?.single" class="btn btn-primary" @click="editing = { item: null }"><Plus :size="15" />{{ t('admin.items.create') }}</button>
    </header>

    <slot name="top" />

    <StateBlock :loading="loading && !items.length" :error="error" @retry="load()">
      <div class="filters" role="group" :aria-label="t('admin.items.filter')">
        <button class="chip" :class="{ on: filter === 'all' }" :aria-pressed="filter === 'all'" @click="filter = 'all'">{{ t('admin.items.fAll', { n: items.length }) }}</button>
        <button class="chip" :class="{ on: filter === 'modified' }" :aria-pressed="filter === 'modified'" @click="filter = 'modified'">{{ t('admin.items.fModified', { n: counts.modified }) }}</button>
        <button class="chip" :class="{ on: filter === 'custom' }" :aria-pressed="filter === 'custom'" @click="filter = 'custom'">{{ t('admin.items.fCustom', { n: counts.custom }) }}</button>
        <button v-if="counts.updates" class="chip" :class="{ on: filter === 'updates' }" :aria-pressed="filter === 'updates'" @click="filter = 'updates'">{{ t('admin.items.fUpdates', { n: counts.updates }) }}</button>
        <button v-if="counts.errors" class="chip" :class="{ on: filter === 'errors' }" :aria-pressed="filter === 'errors'" @click="filter = 'errors'">{{ t('admin.items.fErrors', { n: counts.errors }) }}</button>
      </div>

      <div v-if="!filtered.length" class="empty card">{{ items.length ? t('admin.items.noMatch') : t('admin.items.empty') }}</div>
      <div v-else class="card tablewrap items-table">
        <table class="table">
          <thead>
            <tr>
              <th>{{ t('admin.items.colId') }}</th>
              <th>{{ t('admin.items.colName') }}</th>
              <slot name="head" />
              <th>{{ t('admin.items.colStatus') }}</th>
              <th>{{ t('admin.items.colUpdated') }}</th>
              <th class="actions-col"><span class="sr-only">{{ t('admin.items.colActions') }}</span></th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="r in filtered" :key="r.item.id" :class="{ busy: busy.has(r.item.id), bad: errorIds.has(r.item.id), disabled: r.item.data && r.item.data.enabled === false }">
              <td class="mono idcell"><button class="linklike" @click="editing = { item: r.item }">{{ r.item.id }}</button></td>
              <td>
                <div class="truncate title-cell">{{ r.title }}</div>
                <div v-if="r.subtitle" class="faint truncate sub-cell">{{ r.subtitle }}</div>
                <div v-if="errorIds.has(r.item.id)" class="cell-err" :title="errorIds.get(r.item.id)">{{ errorIds.get(r.item.id) }}</div>
              </td>
              <slot :item="r.item" :patch="patch" :busy="busy.has(r.item.id)" />
              <td>
                <div class="badges">
                  <span v-if="r.item.origin === 'custom'" class="tag tag-outline">{{ t('admin.items.custom') }}</span>
                  <span v-else class="tag tag-nodata">{{ t('admin.items.builtin') }}</span>
                  <span v-if="r.item.modified && r.item.origin !== 'custom'" class="tag tag-risk">{{ t('admin.items.modified') }}</span>
                  <span v-if="r.item.updateAvailable" class="tag tag-accent" :title="t('admin.items.updateHint')"><ArrowUpCircle :size="11" />{{ t('admin.items.update') }}</span>
                  <span v-if="errorIds.has(r.item.id)" class="tag tag-crit">{{ t('admin.items.parseError') }}</span>
                </div>
              </td>
              <td class="faint nowrap" :title="r.item.updatedBy">{{ r.item.modified || r.item.origin === 'custom' ? fmtRelative(r.item.updatedAt) : '—' }}</td>
              <td class="actions">
                <button v-if="r.item.updateAvailable" class="btn btn-sm" :disabled="busy.has(r.item.id)" @click="reset(r.item)"><RotateCcw :size="13" />{{ t('admin.items.resetNew') }}</button>
                <button v-else-if="r.item.hasDefault && r.item.modified" class="btn btn-sm btn-icon btn-ghost" :title="t('admin.items.reset')" :aria-label="t('admin.items.reset')" :disabled="busy.has(r.item.id)" @click="reset(r.item)"><RotateCcw :size="14" /></button>
                <button class="btn btn-sm btn-icon btn-ghost" :title="t('common.edit')" :aria-label="t('common.edit')" @click="editing = { item: r.item }"><Pencil :size="14" /></button>
                <button v-if="r.item.origin === 'custom' && !meta?.single" class="btn btn-sm btn-icon btn-ghost danger" :title="t('common.delete')" :aria-label="t('common.delete')" :disabled="busy.has(r.item.id)" @click="remove(r.item)"><Trash2 :size="14" /></button>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </StateBlock>

    <ConfigItemEditor v-if="editing" :key="editing.item?.id ?? '+'" :kind="kind" :item="editing.item" :items="items" @close="closeEditor" @saved="onSaved" @reset="reset" />
  </section>
</template>
