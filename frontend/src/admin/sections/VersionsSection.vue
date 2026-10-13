<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { Clock, DownloadCloud, RefreshCw } from 'lucide-vue-next'
import * as api from '@/api/endpoints'
import type { ConfigItem, Cycle, VersionsStatus } from '@/api/types'
import { t } from '@/i18n'
import { errorMessage, fmtRelative } from '@/lib/format'
import { ageDays, parseDate } from '@/lib/lifecycle'
import { useDialogs } from '@/stores/dialogs'
import { useToasts } from '@/stores/toasts'
import ConfigItems from './ConfigItems.vue'

const toasts = useToasts()
const dialogs = useDialogs()
const status = ref<VersionsStatus | null>(null)
const list = ref<InstanceType<typeof ConfigItems> | null>(null)
const syncing = ref<Set<string>>(new Set())
const all = ref(false)

const today = computed(() => parseDate(status.value?.today) ?? null)
const staleDays = computed(() => status.value?.staleDays ?? 120)
const lastText = computed(() => {
  const l = status.value?.last
  if (!l) return t('admin.versions.neverSynced')
  return t('admin.versions.lastSync', { when: fmtRelative(l.at), by: l.by === 'auto' ? t('admin.versions.by.auto') : t('admin.versions.by.manual'), ok: l.ok, failed: l.failed.length })
})
const nextText = computed(() => {
  const d = status.value?.autoSyncDays ?? 0
  return d > 0 ? t('admin.versions.autoOn', { n: d }) : t('admin.versions.autoOff')
})

async function loadStatus() {
  try { status.value = await api.admin.versionsStatus() } catch (e) { toasts.error(errorMessage(e)) }
}
onMounted(loadStatus)

function dataOf(item: ConfigItem): { asOf: string; cycles: Cycle[]; source: string } {
  const d = (item.data ?? {}) as { asOf?: string; cycles?: Cycle[]; source?: string }
  return { asOf: d.asOf ?? '', cycles: d.cycles ?? [], source: d.source ?? '' }
}
function age(item: ConfigItem): number | null {
  if (today.value === null) return null
  return ageDays({ id: item.id, source: '', asOf: dataOf(item).asOf, cycles: [] }, today.value)
}
function host(url: string): string {
  try { return new URL(url).host } catch { return url || t('admin.versions.manual') }
}

async function sync(ids: string[]) {
  const key = ids.length ? ids[0] : '*'
  if (ids.length === 0) {
    const ok = await dialogs.confirm({ title: t('admin.versions.syncAllTitle'), text: t('admin.versions.syncAllText'), confirmLabel: t('admin.versions.syncAll') })
    if (!ok) return
    all.value = true
  }
  syncing.value = new Set(syncing.value).add(key)
  try {
    const r = await api.admin.versionsSync(ids)
    if (r.failed) {
      const bad = r.results.filter((x) => !x.ok).slice(0, 3).map((x) => `${x.id}: ${x.error}`).join('; ')
      toasts.error(t('admin.versions.syncPartial', { ok: r.ok, failed: r.failed, why: bad }))
    } else toasts.ok(t('admin.versions.synced', { n: r.ok }))
    await Promise.all([loadStatus(), list.value?.reload(false)])
  } catch (e) {
    toasts.error(errorMessage(e))
  } finally {
    const next = new Set(syncing.value)
    next.delete(key)
    syncing.value = next
    all.value = false
  }
}
</script>

<template>
  <ConfigItems ref="list" kind="catalog.versions">
    <template #top>
    <div class="vstat card" role="status">
      <div class="cell">
        <Clock :size="16" aria-hidden="true" />
        <div>
          <b>{{ t('admin.versions.tables', { n: status?.tables ?? '…' }) }}</b>
          <div class="muted small">{{ status?.oldestDays != null ? t('admin.versions.oldest', { n: status.oldestDays }) : '' }}</div>
        </div>
      </div>
      <div class="cell grow">
        <div>
          <span v-if="status?.stale.length" class="tag tag-risk">{{ t('admin.versions.stale', { n: status.stale.length, days: staleDays }) }}</span>
          <span v-else-if="status" class="tag tag-ok">{{ t('admin.versions.fresh') }}</span>
        </div>
        <div class="muted small">{{ lastText }} · {{ nextText }}</div>
      </div>
      <button class="btn btn-primary" :disabled="syncing.size > 0" @click="sync([])"><DownloadCloud :size="15" />{{ all ? t('admin.versions.syncing') : t('admin.versions.syncAll') }}</button>
    </div>
    </template>
      <template #head>
        <th>{{ t('admin.versions.colAsOf') }}</th>
        <th>{{ t('admin.versions.colLines') }}</th>
        <th>{{ t('admin.versions.colSource') }}</th>
      </template>
      <template #default="{ item }">
        <td class="nowrap">
          <span class="tag" :class="age(item) !== null && age(item)! > staleDays ? 'tag-risk' : 'tag-ok'">{{ dataOf(item).asOf || '—' }}</span>
          <span v-if="age(item) !== null" class="faint small"> · {{ t('admin.versions.ageDays', { n: age(item) as number }) }}</span>
        </td>
        <td class="mono">{{ dataOf(item).cycles.length }}</td>
        <td class="nowrap">
          <span class="faint small" :title="dataOf(item).source">{{ host(dataOf(item).source) }}</span>
          <button v-if="dataOf(item).source.startsWith('http')" class="btn btn-sm btn-icon btn-ghost" :title="t('admin.versions.syncOne')" :aria-label="t('admin.versions.syncOne')" :disabled="syncing.has(item.id) || syncing.has('*')" @click="sync([item.id])">
            <RefreshCw :size="14" :class="{ spin: syncing.has(item.id) }" />
          </button>
        </td>
      </template>
  </ConfigItems>
</template>

<style scoped>
.vstat { display: flex; align-items: center; gap: 22px; margin: 0 0 14px; padding: 12px 16px; }
.vstat .cell { display: flex; align-items: center; gap: 10px; min-width: 0; }
.vstat .cell > div { display: flex; flex-direction: column; gap: 3px; }
.small { font-size: 12px; }
.grow { flex: 1; }
.spin { animation: spin 1s linear infinite; }
@keyframes spin { to { transform: rotate(360deg); } }
@media (max-width: 900px) { .vstat { flex-wrap: wrap; gap: 12px; } }
</style>
