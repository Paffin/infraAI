<script setup lang="ts">
import { computed, ref } from 'vue'
import { ChevronDown } from 'lucide-vue-next'
import type { Cycle, PropDef, Scalar } from '@/api/types'
import { lt, t } from '@/i18n'
import type { VersionIndex, Verdict } from '@/lib/lifecycle'
import { cycleVerdict, viewVersion, type VersionRef } from '@/lib/product'

const props = defineProps<{
  def: PropDef
  /** version declared by the user (undefined = not declared) */
  value: Scalar | undefined
  /** the version the catalog assumes */
  fallback?: Scalar
  disabled?: boolean
  flash?: boolean
  /** the table that judges this version (absent: a plain field) */
  table?: VersionRef
  versions: VersionIndex | null
}>()
const emit = defineEmits<{ change: [value: Scalar | undefined] }>()

const open = ref(false)
const root = ref<HTMLElement | null>(null)
const typed = ref('')

const declared = computed(() => props.value !== undefined && props.value !== null && props.value !== '')
const text = computed(() => (declared.value ? String(props.value) : ''))
const view = computed(() => (props.table && props.versions ? viewVersion(props.versions, props.table.set, props.value) : null))
const shown = computed(() => view.value?.lifecycle ?? null)

const CHIP: Record<Verdict, string> = { supported: 'tag-ok', soon: 'tag-risk', maintenance: 'tag-nodata', eol: 'tag-crit', unknown: 'tag-nodata' }
const DOT: Record<Verdict, string> = { supported: 'var(--ok-dot)', soon: 'var(--risk-dot)', maintenance: 'var(--info-dot)', eol: 'var(--crit)', unknown: 'var(--faint)' }

const chip = computed(() => {
  const v = view.value
  if (!v) return null
  if (!declared.value) return { cls: 'tag-nodata', text: t('version.undeclared') }
  const l = v.lifecycle
  if (!l) return { cls: 'tag-nodata', text: t('version.unknown') }
  const date = l.eol ?? ''
  switch (v.verdict) {
    case 'eol': return { cls: CHIP.eol, text: date ? t('version.eol', { date }) : t('version.eolNoDate') }
    case 'soon': return { cls: CHIP.soon, text: t('version.soon', { n: Math.max(0, l.daysToEol ?? 0) }) }
    case 'maintenance': return { cls: CHIP.maintenance, text: date ? t('version.maintenance', { date }) : t('version.maintenanceNoDate') }
    case 'supported': return { cls: CHIP.supported, text: date ? t('version.supportedUntil', { date }) : t('version.supported') }
    default: return { cls: CHIP.unknown, text: t('version.unknown') }
  }
})

const rec = computed(() => {
  const r = view.value?.recommended
  if (!r) return null
  // nothing to recommend when the node already runs that line
  return shown.value && shown.value.cycle === r.cycle ? null : r
})

const cycles = computed<Cycle[]>(() => props.table?.set.cycles ?? [])
const filtered = computed(() => {
  const q = typed.value.trim().toLowerCase()
  if (!q) return cycles.value
  const f = cycles.value.filter((c) => c.cycle.toLowerCase().startsWith(q) || q.startsWith(c.cycle.toLowerCase()))
  return f.length ? f : cycles.value
})
const placeholder = computed(() => (props.fallback !== undefined && props.fallback !== null ? t('inspector.default', { v: String(props.fallback) }) : t('inspector.unset')))

function cycleDate(c: Cycle): string {
  const e = c.eol
  if (typeof e === 'string') return view.value && props.versions && props.versions.today > 0 && cycleVerdict(props.versions, props.table!.set, c) === 'eol' ? t('version.over') : t('version.until', { date: e })
  return e === true ? t('version.over') : ''
}

/** Opening the list shows every line; only typing narrows it. */
function show() {
  if (!props.table || props.disabled) return
  typed.value = ''
  open.value = true
}

function pick(c: string) {
  emit('change', c)
  typed.value = ''
  open.value = false
}
function onInput(e: Event) {
  const v = (e.target as HTMLInputElement).value
  typed.value = v
  emit('change', v === '' ? undefined : v)
  if (props.table) open.value = true
}
function onFocusOut(e: FocusEvent) {
  if (!root.value?.contains(e.relatedTarget as Node | null)) {
    open.value = false
    typed.value = ''
  }
}
const latest = computed(() => (shown.value?.latest && shown.value.latest !== text.value ? shown.value.latest : null))
const staleText = computed(() => (view.value?.stale && view.value.ageDays !== null ? t('version.stale', { n: view.value.ageDays }) : ''))
</script>

<template>
  <div ref="root" class="vf" :class="{ assumed: !declared, flash }" :data-prop="def.key" @focusout="onFocusOut" @keydown.esc="open = false">
    <label class="prop" :title="def.help ? lt(def.help) : undefined">
      <span class="lbl">{{ lt(def.label) }}</span>
      <span class="ctl">
        <input class="in mono" type="text" autocomplete="off" :disabled="disabled" :value="declared ? text : ''" :placeholder="placeholder" role="combobox" :aria-expanded="open" aria-haspopup="listbox"
          @input="onInput" @focus="show" @click="open || show()" @keydown.down.prevent="show" />
        <button v-if="table && !disabled" type="button" class="chev" tabindex="-1" :aria-label="t('version.pick')" @mousedown.prevent="open ? (open = false) : show()"><ChevronDown :size="13" /></button>
      </span>
    </label>

    <div v-if="chip" class="meta">
      <span class="tag" :class="chip.cls">{{ chip.text }}</span>
      <span v-if="shown?.lts" class="tag tag-nodata">{{ t('version.lts') }}</span>
      <span v-if="latest" class="faint">{{ t('version.latest', { v: latest }) }}</span>
    </div>
    <div v-if="rec && !disabled" class="reco">
      <span>{{ t('version.recommended', { v: rec.cycle }) }}</span>
      <button type="button" class="use" @click="pick(rec.cycle)">{{ t('version.use') }}</button>
    </div>
    <div v-if="staleText" class="meta"><span class="tag tag-risk">{{ staleText }}</span></div>
    <div v-if="!table && versions && (def.key === 'version' || def.lifecycle)" class="meta"><span class="faint">{{ t('version.noTable') }}</span></div>

    <ul v-if="open && table" class="list" role="listbox" :aria-label="t('version.lines')">
      <li v-for="c in filtered" :key="c.cycle">
        <button type="button" class="opt" role="option" :aria-selected="shown?.cycle === c.cycle" :class="{ on: shown?.cycle === c.cycle }" @mousedown.prevent="pick(c.cycle)">
          <i class="dot" :style="{ background: versions ? DOT[cycleVerdict(versions, table.set, c)] : 'var(--faint)' }" />
          <b class="mono">{{ c.cycle }}</b>
          <span v-if="c.lts" class="lts">{{ t('version.lts') }}</span>
          <span class="when faint">{{ cycleDate(c) }}</span>
        </button>
      </li>
      <li class="asof faint">{{ t('version.asOf', { date: table.set.asOf || '—' }) }}</li>
    </ul>
  </div>
</template>

<style scoped>
.vf { position: relative; display: flex; flex-direction: column; gap: 3px; padding: 2px 0; flex: none; }
.prop { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1.05fr); gap: 10px; align-items: center; min-height: 30px; font-size: 12.5px; cursor: default; }
.lbl { color: var(--muted); line-height: 1.25; }
.ctl { position: relative; display: block; }
.in { width: 100%; height: 28px; padding: 0 22px 0 8px; border: 1px solid transparent; border-radius: var(--radius-sm); background: transparent; text-align: right; font-size: 12px; color: var(--text); }
.in:hover:not(:disabled) { border-color: var(--line-strong); background: var(--surface); }
.in:focus { outline: none; border-color: var(--accent); background: var(--surface); box-shadow: 0 0 0 3px var(--accent-soft); }
.in::placeholder { color: var(--faint); }
.in:disabled { cursor: default; -webkit-text-fill-color: currentColor; opacity: 1; }
.assumed .in:not(:focus) { color: var(--faint); }
.chev { position: absolute; right: 3px; top: 5px; width: 18px; height: 18px; border: 0; background: transparent; color: var(--faint); border-radius: 4px; display: inline-flex; align-items: center; justify-content: center; }
.chev:hover { color: var(--text); background: var(--surface-sunken); }
.meta { display: flex; flex-wrap: wrap; gap: 4px 6px; align-items: center; justify-content: flex-end; font-size: 11.5px; }
.meta .tag { max-width: 100%; white-space: normal; text-align: right; line-height: 1.3; }
.reco { display: flex; gap: 8px; align-items: center; justify-content: flex-end; font-size: 11.5px; color: var(--muted); }
.use { border: 0; background: transparent; color: var(--accent); font-size: 11.5px; padding: 0 2px; border-radius: 3px; text-decoration: underline; text-underline-offset: 2px; }
.use:hover { color: var(--accent-hover); }
.list { position: absolute; z-index: 60; right: 0; top: 34px; width: min(100%, 230px); max-height: 232px; overflow: auto; margin: 0; padding: 4px; list-style: none; background: var(--surface); border: 1px solid var(--line-strong); border-radius: var(--radius); box-shadow: var(--shadow-2); }
.opt { width: 100%; display: flex; align-items: center; gap: 8px; height: 28px; padding: 0 8px; border: 0; background: transparent; border-radius: var(--radius-sm); font-size: 12px; text-align: left; color: var(--text); }
.opt:hover, .opt.on { background: var(--accent-soft); }
.opt .dot { flex: none; width: 8px; height: 8px; border-radius: 50%; }
.opt .when { margin-left: auto; font-size: 11px; }
.lts { font-size: 9.5px; letter-spacing: .06em; padding: 1px 4px; border-radius: 3px; background: var(--nodata-bg); color: var(--nodata); }
.asof { padding: 6px 8px 3px; font-size: 10.5px; border-top: 1px solid var(--line); margin-top: 3px; }
.faint { color: var(--faint); }
.flash { animation: flash 2.2s ease-out; border-radius: var(--radius-sm); }
@keyframes flash { 0%, 55% { background: var(--accent-soft); box-shadow: 0 0 0 4px var(--accent-soft); } 100% { background: transparent; box-shadow: 0 0 0 4px transparent; } }
</style>
