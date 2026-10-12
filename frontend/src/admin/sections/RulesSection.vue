<script setup lang="ts">
import type { ConfigItem } from '@/api/types'
import { t } from '@/i18n'
import { useToasts } from '@/stores/toasts'
import { isPlainObject } from '../lib/json'
import ConfigItems from './ConfigItems.vue'

type Patch = (item: ConfigItem, data: unknown) => Promise<boolean>

const toasts = useToasts()
const data = (i: ConfigItem): Record<string, unknown> => (isPlainObject(i.data) ? i.data : {})
const enabled = (i: ConfigItem) => data(i).enabled !== false
const severity = (i: ConfigItem) => (data(i).severity === 'critical' ? 'critical' : 'risk')

function toggle(i: ConfigItem, patch: Patch) {
  return patch(i, { ...data(i), enabled: !enabled(i) })
}

function setSeverity(i: ConfigItem, patch: Patch, e: Event) {
  const el = e.target as HTMLSelectElement
  if (el.value === data(i).severity) return
  return patch(i, { ...data(i), severity: el.value })
}

async function setWeight(i: ConfigItem, patch: Patch, e: Event) {
  const el = e.target as HTMLInputElement
  const raw = el.value.trim()
  const cur = data(i).weight
  if (raw === '' && (cur === undefined || cur === null)) return
  const next: Record<string, unknown> = { ...data(i) }
  if (raw === '') {
    delete next.weight
  } else {
    const n = Number(raw)
    if (!Number.isFinite(n) || n < 0) {
      toasts.error(t('admin.rules.badWeight'))
      el.value = cur == null ? '' : String(cur)
      return
    }
    if (n === cur) return
    next.weight = n
  }
  const ok = await patch(i, next)
  if (!ok) el.value = cur == null ? '' : String(cur)
}
</script>

<template>
  <ConfigItems kind="rule">
    <template #head>
      <th>{{ t('admin.rules.enabled') }}</th>
      <th>{{ t('admin.rules.severity') }}</th>
      <th>{{ t('admin.rules.weight') }}</th>
    </template>
    <template #default="{ item, patch, busy }">
      <td>
        <button
          type="button" class="switch" role="switch" :aria-checked="enabled(item)" :aria-label="t('admin.rules.toggle', { id: item.id })"
          :disabled="busy" @click="toggle(item, patch)"
        ><span class="switch-knob" /></button>
      </td>
      <td>
        <select class="select input-sm sev" :class="severity(item)" :value="severity(item)" :disabled="busy" :aria-label="t('admin.rules.severityOf', { id: item.id })" @change="setSeverity(item, patch, $event)">
          <option value="risk">{{ t('admin.rules.sev.risk') }}</option>
          <option value="critical">{{ t('admin.rules.sev.critical') }}</option>
        </select>
      </td>
      <td>
        <input
          class="input input-sm weight" type="number" step="any" min="0" :value="data(item).weight ?? ''" :placeholder="t('admin.rules.weightAuto')"
          :disabled="busy" :aria-label="t('admin.rules.weightOf', { id: item.id })" @change="setWeight(item, patch, $event)"
        />
      </td>
    </template>
  </ConfigItems>
</template>
