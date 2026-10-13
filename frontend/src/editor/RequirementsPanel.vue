<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { lt, t } from '@/i18n'
import { splitSeconds, toSeconds } from '@/lib/format'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'
import { useUi } from '@/stores/ui'

const app = useApp()
const editor = useEditor()
const ui = useUi()
const req = computed(() => editor.arch.requirements)
const dis = computed(() => editor.readonly)
const tolerance = computed(() => app.boot?.engine.defaultFailureTolerance ?? 'site')
/** The failure ladder comes from the engine settings (delivered by /bootstrap). */
const levels = computed(() => [...(app.boot?.engine.toleranceLevels ?? [])].sort((a, b) => a.rank - b.rank))
const levelName = (id: string) => { const l = levels.value.find((x) => x.id === id); return l ? lt(l.name) : t('req.failureTolerance.' + id) }
const levelHelp = computed(() => { const l = levels.value.find((x) => x.id === (req.value.failureTolerance ?? tolerance.value)); return l ? lt(l.help) : '' })
const maxSiteFailures = computed(() => app.boot?.engine.maxSiteFailures ?? 4)
/** Several sites at once only make sense when sites are the failure domain being asked about. */
const asksSites = computed(() => { const cur = levels.value.find((x) => x.id === (req.value.failureTolerance ?? tolerance.value)); return !cur ? true : cur.domain === 'site' || cur.domain === 'geo' })
const symbol = computed(() => app.boot?.engine.symbol ?? '')
const compliance = ref('')

type NumKey = 'rpsPeak' | 'growthPercent' | 'availabilitySlo' | 'budgetMonthly'
function setNum(key: NumKey, e: Event) {
  const raw = (e.target as HTMLInputElement).value
  editor.setRequirement(key, raw === '' ? undefined : Number(raw))
}

type DurKey = 'rtoSeconds' | 'rpoSeconds'
const unit = ref<Record<DurKey, 'seconds' | 'minutes' | 'hours'>>({ rtoSeconds: splitSeconds(req.value.rtoSeconds ?? 0).unit, rpoSeconds: splitSeconds(req.value.rpoSeconds ?? 0).unit })
const durValue = (k: DurKey) => {
  const s = req.value[k]
  if (s === undefined || s === null) return ''
  return unit.value[k] === 'hours' ? s / 3600 : unit.value[k] === 'minutes' ? s / 60 : s
}
function setDur(k: DurKey, e: Event) {
  const raw = (e.target as HTMLInputElement).value
  editor.setRequirement(k, raw === '' ? undefined : toSeconds(Number(raw), unit.value[k]))
}
/** Changing the unit keeps the number on screen ("5 с" → "5 мин") and recomputes the stored seconds. */
function setUnit(k: DurKey, u: 'seconds' | 'minutes' | 'hours') {
  const shown = durValue(k)
  unit.value[k] = u
  if (shown !== '') editor.setRequirement(k, toSeconds(Number(shown), u))
}

function setTolerance(e: Event) {
  const v = (e.target as HTMLSelectElement).value
  editor.setRequirement('failureTolerance', v || undefined)
}
function setSiteFailures(e: Event) {
  const raw = (e.target as HTMLInputElement).value
  editor.setRequirement('siteFailures', raw === '' ? undefined : Math.max(1, Math.min(maxSiteFailures.value, Math.round(Number(raw)))))
}

// ------------------------------------------------------------------------- "fill this in" requests
const root = ref<HTMLElement | null>(null)
const flashKey = ref('')
let flashTimer: ReturnType<typeof setTimeout> | undefined
const attrs = (key: string) => ({ 'data-req': key, class: flashKey.value === key ? 'flash' : undefined })

/** A "no data" finding named a requirement: scroll to the field, focus it and flash it once. */
async function applyFocus() {
  const r = ui.focusRequest
  if (!r || r.need.target !== 'requirements') return
  ui.focusRequest = null
  await nextTick()
  const row = root.value?.querySelector<HTMLElement>(`[data-req="${CSS.escape(r.need.prop)}"]`)
  if (!row) return
  row.scrollIntoView({ block: 'center', behavior: 'smooth' })
  row.querySelector<HTMLElement>('input, select, textarea')?.focus({ preventScroll: true })
  flashKey.value = r.need.prop
  clearTimeout(flashTimer)
  flashTimer = setTimeout(() => { flashKey.value = '' }, 2300)
}
watch(() => ui.focusRequest, () => void applyFocus())
onMounted(() => void nextTick(applyFocus))

function addCompliance() {
  const items = compliance.value.split(/[,;\n]/).map((s) => s.trim()).filter(Boolean)
  if (!items.length) return
  editor.setRequirement('compliance', [...new Set([...(req.value.compliance ?? []), ...items])])
  compliance.value = ''
}
function removeCompliance(x: string) {
  editor.setRequirement('compliance', (req.value.compliance ?? []).filter((y) => y !== x))
}
</script>

<template>
  <div ref="root" class="reqs">
    <h3>{{ t('req.title') }}</h3>
    <p class="muted intro">{{ t('req.intro') }}</p>

    <div class="field" v-bind="attrs('rpsPeak')">
      <label>{{ t('req.rpsPeak') }}</label>
      <input class="input mono" type="number" min="0" :disabled="dis" :value="req.rpsPeak ?? ''" @input="setNum('rpsPeak', $event)" />
    </div>
    <div class="field" v-bind="attrs('growthPercent')">
      <label>{{ t('req.growthPercent') }}</label>
      <input class="input mono" type="number" min="0" :disabled="dis" :value="req.growthPercent ?? ''" @input="setNum('growthPercent', $event)" />
    </div>

    <div v-for="k in (['rtoSeconds', 'rpoSeconds'] as const)" :key="k" class="field" v-bind="attrs(k)">
      <label>{{ t('req.' + k) }}</label>
      <div class="row">
        <input class="input mono" type="number" min="0" step="any" :disabled="dis" :value="durValue(k)" @input="setDur(k, $event)" />
        <select class="select" style="width: 110px" :disabled="dis" :value="unit[k]" @change="setUnit(k, ($event.target as HTMLSelectElement).value as 'seconds' | 'minutes' | 'hours')">
          <option value="seconds">{{ t('req.unit.seconds') }}</option>
          <option value="minutes">{{ t('req.unit.minutes') }}</option>
          <option value="hours">{{ t('req.unit.hours') }}</option>
        </select>
      </div>
      <span v-if="k === 'rpoSeconds'" class="hint">{{ t('req.rpoSeconds.hint') }}</span>
    </div>

    <div class="field" v-bind="attrs('availabilitySlo')">
      <label>{{ t('req.availabilitySlo') }}</label>
      <input class="input mono" type="number" min="0" max="100" step="0.001" :disabled="dis" :value="req.availabilitySlo ?? ''" @input="setNum('availabilitySlo', $event)" />
    </div>
    <div class="field" v-bind="attrs('budgetMonthly')">
      <label>{{ t('req.budgetMonthly') }}, {{ symbol }}</label>
      <input class="input mono" type="number" min="0" :disabled="dis" :value="req.budgetMonthly ?? ''" @input="setNum('budgetMonthly', $event)" />
    </div>
    <div class="field" v-bind="attrs('failureTolerance')">
      <label>{{ t('req.failureTolerance') }}</label>
      <select class="select" :disabled="dis" :value="req.failureTolerance ?? ''" @change="setTolerance">
        <option value="">{{ t('inspector.default', { v: levelName(tolerance) }) }}</option>
        <option v-for="l in levels" :key="l.id" :value="l.id">{{ levelName(l.id) }}</option>
      </select>
      <span v-if="levelHelp" class="hint">{{ levelHelp }}</span>
    </div>
    <div v-if="asksSites" class="field" v-bind="attrs('siteFailures')">
      <label>{{ t('req.siteFailures') }}</label>
      <input class="input mono" type="number" min="1" :max="maxSiteFailures" step="1" :disabled="dis" :value="req.siteFailures ?? ''" placeholder="1" @input="setSiteFailures" />
      <span class="hint">{{ t('req.siteFailures.hint') }}</span>
    </div>
    <div class="field" v-bind="attrs('compliance')">
      <label>{{ t('req.compliance') }}</label>
      <div class="chips">
        <span v-for="c in req.compliance ?? []" :key="c" class="chip">{{ c }}<button v-if="!dis" class="x" :aria-label="t('common.delete')" @click="removeCompliance(c)">×</button></span>
      </div>
      <input v-if="!dis" v-model="compliance" class="input" :placeholder="t('req.compliance.add')" @keydown.enter.prevent="addCompliance" @blur="addCompliance" />
    </div>
    <div class="field" v-bind="attrs('notes')">
      <label>{{ t('req.notes') }}</label>
      <textarea class="textarea" rows="3" :disabled="dis" :value="req.notes ?? ''" @input="editor.setRequirement('notes', ($event.target as HTMLTextAreaElement).value)" />
    </div>
  </div>
</template>

<style scoped>
.reqs { padding: 16px 20px 28px; display: flex; flex-direction: column; gap: 14px; overflow: auto; }
h3 { font-size: 16px; }
.intro { margin: -6px 0 0; font-size: 12.5px; }
.field { border-radius: var(--radius-sm); }
.field.flash { animation: flash 2.2s ease-out; }
@keyframes flash { 0%, 55% { background: var(--accent-soft); box-shadow: 0 0 0 6px var(--accent-soft); } 100% { background: transparent; box-shadow: 0 0 0 6px transparent; } }
.chips { display: flex; flex-wrap: wrap; gap: 6px; }
.x { border: 0; background: transparent; color: var(--muted); font-size: 15px; line-height: 1; padding: 0; }
</style>
