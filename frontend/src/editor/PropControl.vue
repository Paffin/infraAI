<script setup lang="ts">
import { computed } from 'vue'
import type { PropDef, Scalar } from '@/api/types'
import { lt, t } from '@/i18n'

const props = defineProps<{
  def: PropDef
  /** value declared by the user (undefined/null = not declared) */
  value: Scalar | undefined
  /** value the engine assumes when nothing is declared */
  fallback?: Scalar
  disabled?: boolean
  /** choices for `unit_ref` properties: [value, label] */
  refs?: [string, string][]
  /** briefly highlight the row (the user was sent here to fill it in) */
  flash?: boolean
}>()
const emit = defineEmits<{ change: [value: Scalar | undefined] }>()

const declared = computed(() => props.value !== undefined && props.value !== null && props.value !== '')
const placeholder = computed(() => (props.fallback !== undefined && props.fallback !== null ? t('inspector.default', { v: String(props.fallback) }) : t('inspector.unset')))
const tri = computed(() => (props.value === true ? 'true' : props.value === false ? 'false' : ''))

function onNumber(e: Event) {
  const raw = (e.target as HTMLInputElement).value
  if (raw === '') return emit('change', undefined)
  const n = Number(raw)
  if (Number.isFinite(n)) emit('change', n)
}
function onText(e: Event) {
  const v = (e.target as HTMLInputElement).value
  emit('change', v === '' ? undefined : v)
}
function onEnum(e: Event) {
  const v = (e.target as HTMLSelectElement).value
  emit('change', v === '' ? undefined : v)
}
function onBool(e: Event) {
  const v = (e.target as HTMLSelectElement).value
  emit('change', v === '' ? undefined : v === 'true')
}
</script>

<template>
  <label class="prop" :class="{ assumed: !declared, flash }" :data-prop="def.key" :title="def.help ? lt(def.help) : undefined">
    <span class="lbl">{{ lt(def.label) }}<span v-if="def.unit && def.type === 'number'" class="unit"> · {{ def.unit }}</span></span>
    <span class="ctl">
      <input
        v-if="def.type === 'number'" class="in mono" type="number" :min="def.min ?? undefined" :max="def.max ?? undefined" step="any" :disabled="disabled"
        :value="declared ? (value as number) : ''" :placeholder="placeholder" @input="onNumber"
      />
      <select v-else-if="def.type === 'boolean'" class="in" :disabled="disabled" :value="tri" @change="onBool">
        <option value="">{{ fallback !== undefined && fallback !== null ? t('inspector.default', { v: fallback ? t('common.yes') : t('common.no') }) : t('inspector.unset') }}</option>
        <option value="true">{{ t('common.yes') }}</option>
        <option value="false">{{ t('common.no') }}</option>
      </select>
      <select v-else-if="def.type === 'enum'" class="in" :disabled="disabled" :value="declared ? String(value) : ''" @change="onEnum">
        <option value="">{{ placeholder }}</option>
        <option v-for="o in def.options ?? []" :key="o.value" :value="o.value">{{ lt(o.label) }}</option>
      </select>
      <select v-else-if="def.type === 'unit_ref'" class="in" :disabled="disabled" :value="declared ? String(value) : ''" @change="onEnum">
        <option value="">{{ t('inspector.unset') }}</option>
        <option v-for="[v, l] in refs ?? []" :key="v" :value="v">{{ l }}</option>
        <option v-if="declared && !(refs ?? []).some(([v]) => v === value)" :value="String(value)">{{ value }}</option>
      </select>
      <input v-else class="in" type="text" :disabled="disabled" :value="declared ? String(value) : ''" :placeholder="placeholder" @input="onText" />
    </span>
  </label>
</template>

<style scoped>
.prop { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1.05fr); gap: 10px; align-items: center; min-height: 30px; font-size: 12.5px; cursor: default; flex: none; }
.lbl { color: var(--muted); line-height: 1.25; }
.unit { color: var(--faint); }
.ctl { display: block; }
.in { width: 100%; height: 28px; padding: 0 8px; border: 1px solid transparent; border-radius: var(--radius-sm); background: transparent; text-align: right; font-size: 12.5px; color: var(--text); }
.in.mono { font-family: var(--mono); font-size: 12px; }
select.in { text-align-last: right; padding-right: 2px; }
.in:hover:not(:disabled) { border-color: var(--line-strong); background: var(--surface); }
.in:focus { outline: none; border-color: var(--accent); background: var(--surface); box-shadow: 0 0 0 3px var(--accent-soft); }
.in::placeholder { color: var(--faint); }
.assumed .in:not(:focus) { color: var(--faint); }
.flash { animation: flash 2.2s ease-out; border-radius: var(--radius-sm); }
@keyframes flash { 0%, 55% { background: var(--accent-soft); box-shadow: 0 0 0 4px var(--accent-soft); } 100% { background: transparent; box-shadow: 0 0 0 4px transparent; } }
.in:disabled { cursor: default; opacity: 1; -webkit-text-fill-color: currentColor; }
</style>
