<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { Check, Info, Minus, Plus, TriangleAlert } from 'lucide-vue-next'
import * as api from '@/api/endpoints'
import { ApiError } from '@/api/client'
import type { Finding, PlacementLayer, PlacementOptions, PlacementPattern, PlacementPreview, PlacementRequest, PlacementVariant } from '@/api/types'
import Modal from '@/components/Modal.vue'
import { lang, lt, t } from '@/i18n'
import { errorMessage, fmtDuration, fmtMoney, fmtPct } from '@/lib/format'
import { findingText } from '@/lib/findings'
import { comparisonSites } from '@/lib/placementMap'
import { scoreTone } from '@/lib/score'
import { useApp } from '@/stores/app'
import { useDialogs } from '@/stores/dialogs'
import { useEditor } from '@/stores/editor'
import { useToasts } from '@/stores/toasts'
import SiteMap from './SiteMap.vue'

const props = withDefaults(defineProps<{ mode?: 'new' | 'replace' }>(), { mode: 'new' })
const emit = defineEmits<{ close: [] }>()

const app = useApp()
const router = useRouter()
const toasts = useToasts()
const dialogs = useDialogs()
const editor = useEditor()

const options = ref<PlacementOptions | null>(null)
const loadError = ref('')
const patternId = ref('')
const sites = ref(3)
const regions = ref(1)
const racks = ref(1)
/** `null` = the pattern decides (as many as the number of sites allows) */
const failures = ref<number | null>(null)
const siteType = ref('')
const layerOn = ref<Record<string, boolean>>({})
const kinds = ref<Record<string, string>>({})
/** layer id → variant of the chosen product ('' = the product's default) */
const variants = ref<Record<string, string>>({})
const over = ref<{ rpsPeak?: number; rtoSeconds?: number; rpoSeconds?: number; budgetMonthly?: number }>({})
const name = ref('')
/** The suggested name follows the pattern and the number of sites until the user types their own. */
const nameTouched = ref(false)
const busy = ref(false)

const pattern = computed<PlacementPattern | undefined>(() => options.value?.patterns.find((p) => p.id === patternId.value))
const stack = computed(() => options.value?.stacks.find((s) => s.id === pattern.value?.stack))
const optionalLayers = computed(() => stack.value?.layers.filter((l) => l.optional) ?? [])
const UNBOUNDED_SITES = 100
const siteRange = computed(() => ({ min: pattern.value?.minSites ?? 1, max: Math.min(pattern.value?.maxSites ?? UNBOUNDED_SITES, UNBOUNDED_SITES) }))
const quickSites = computed(() => {
  const { min, max } = siteRange.value
  return [...new Set([min, 2, 3, 5, 7, 10, max])].filter((n) => n >= min && n <= max && n <= 20).sort((a, b) => a - b)
})
const maxFailures = computed(() => Math.max(1, Math.min(options.value?.maxSiteFailures ?? 4, sites.value - 1)))
const asksSites = computed(() => !!pattern.value && pattern.value.tolerance !== 'rack' && sites.value >= 2)
const regionsOn = computed(() => pattern.value?.regions.mode === 'count')
const racksOn = computed(() => (pattern.value?.racks.max ?? 1) > 1)
const siteTypeName = (id: string) => lt(app.index?.zoneTypes.get(id)?.name) || id
const kindName = (id: string) => lt(app.index?.kinds.get(id)?.name) || id
const variantName = (kind: string, id: string) => lt(app.index?.kinds.get(kind)?.variants?.find((v) => v.id === id)?.name) || id
/** The product chosen for a layer, with the ways to deploy it. */
const productOf = (layerId: string) => {
  const l = stack.value?.layers.find((x) => x.id === layerId)
  const kind = kinds.value[layerId] ?? l?.kind
  return l?.products.find((p) => p.kind === kind)
}
function setKind(layerId: string, kind: string) {
  kinds.value[layerId] = kind
  variants.value[layerId] = ''
}
/** A quorum layer (a DCS) that none of the chosen products uses: managed services and clusters that vote by themselves do not need one. */
function dcsUnused(l: PlacementLayer): boolean {
  const users = (stack.value?.layers ?? []).filter((x) => x.dcs === l.id && (!x.optional || layerOn.value[x.id]))
  if (!users.length) return false
  return users.every((x) => {
    const kind = kinds.value[x.id] ?? x.kind
    const def = app.index?.kinds.get(kind)?.props.find((p) => p.key === 'dcs')
    if (!def) return true
    if (!def.variants?.length) return false
    const chosen = variants.value[x.id] || productOf(x.id)?.variants.find((v) => v.default)?.id
    return !chosen || !def.variants.includes(chosen)
  })
}

// ------------------------------------------------------------------------------------------ state ⇄ request
function resetToPattern(p: PlacementPattern) {
  sites.value = p.defaultSites
  regions.value = p.regions.default
  racks.value = p.racks.default
  failures.value = null
  siteType.value = p.siteType
  over.value = {}
  const st = options.value?.stacks.find((s) => s.id === p.stack)
  layerOn.value = Object.fromEntries((st?.layers ?? []).filter((l) => l.optional).map((l) => [l.id, l.default]))
  kinds.value = Object.fromEntries((st?.layers ?? []).filter((l) => l.products.length > 1).map((l) => [l.id, l.kind]))
  variants.value = {}
}

function pickPattern(id: string) {
  if (id === patternId.value) return
  patternId.value = id
  const p = options.value?.patterns.find((x) => x.id === id)
  if (p) resetToPattern(p)
}

function setSites(n: number) {
  const { min, max } = siteRange.value
  sites.value = Math.max(min, Math.min(max, Math.round(n) || min))
  regions.value = Math.min(regions.value, sites.value)
  if (failures.value !== null) failures.value = Math.min(failures.value, maxFailures.value)
}
const step = (d: number) => setSites(sites.value + d)

const request = computed<PlacementRequest | null>(() => {
  const p = pattern.value
  if (!p) return null
  const r: PlacementRequest = { pattern: p.id, sites: sites.value, layers: optionalLayers.value.filter((l) => layerOn.value[l.id]).map((l) => l.id) }
  if (regionsOn.value) r.regions = Math.min(regions.value, sites.value)
  if (racksOn.value) r.racks = racks.value
  if (asksSites.value && failures.value !== null) r.siteFailures = Math.min(failures.value, maxFailures.value)
  if (siteType.value && siteType.value !== p.siteType) r.siteType = siteType.value
  const k = Object.fromEntries(Object.entries(kinds.value).filter(([id, kind]) => stack.value?.layers.find((l) => l.id === id)?.kind !== kind))
  if (Object.keys(k).length) r.kinds = k
  const on = (id: string) => optionalLayers.value.find((l) => l.id === id) ? !!layerOn.value[id] : true
  const v = Object.fromEntries(Object.entries(variants.value).filter(([id, variant]) => variant && on(id) && productOf(id)?.variants.some((x) => x.id === variant)))
  if (Object.keys(v).length) r.variants = v
  const req = Object.fromEntries(Object.entries(over.value).filter(([, v]) => typeof v === 'number' && Number.isFinite(v)))
  if (Object.keys(req).length) r.requirements = req
  return r
})

// ------------------------------------------------------------------------------------------ preview
const preview = ref<PlacementPreview | null>(null)
const previewError = ref('')
const computing = ref(false)
let previewCtl: AbortController | undefined
let previewTimer: ReturnType<typeof setTimeout> | undefined

function codeOf(e: unknown): string {
  return e instanceof ApiError ? ((e.details as { code?: string } | undefined)?.code ?? '') : ''
}
function explain(e: unknown): string {
  const code = codeOf(e)
  const key = 'placement.error.' + code
  const base = code ? t(key) : t('placement.error.generic')
  const extra = code === 'too_many_nodes' ? ' ' + t('placement.limit', { n: options.value?.maxNodes ?? '' }) : ''
  // an unknown code falls back to the generic text plus what the server said
  return (base === key ? t('placement.error.generic') + ' ' + errorMessage(e) : base) + extra
}

async function refresh() {
  const req = request.value
  if (!req) return
  previewCtl?.abort()
  const ctl = (previewCtl = new AbortController())
  computing.value = true
  try {
    const r = await api.placement.preview(req, lang.value, ctl.signal)
    if (ctl.signal.aborted) return
    preview.value = r
    previewError.value = ''
  } catch (e) {
    if ((e as Error)?.name === 'AbortError') return
    previewError.value = explain(e)
  } finally {
    if (previewCtl === ctl) computing.value = false
  }
}

const compare = ref<PlacementVariant[]>([])
const comparing = ref(false)
let compareCtl: AbortController | undefined
let compareTimer: ReturnType<typeof setTimeout> | undefined

async function refreshCompare() {
  const req = request.value
  const p = pattern.value
  if (!req || !p) return
  compareCtl?.abort()
  const ctl = (compareCtl = new AbortController())
  comparing.value = true
  try {
    const counts = comparisonSites(siteRange.value.min, p.maxSites ?? undefined, 12)
    if (!counts.includes(sites.value)) counts.push(sites.value)
    counts.sort((a, b) => a - b)
    const base = { ...req }
    delete base.sites
    delete base.siteFailures // the number of tolerated losses follows the number of sites in every variant
    const r = await api.placement.compare(base, counts.map((n) => ({ sites: n, regions: Math.min(req.regions ?? 1, n) })), lang.value, ctl.signal)
    if (!ctl.signal.aborted) compare.value = r.variants
  } catch (e) {
    if ((e as Error)?.name !== 'AbortError') compare.value = []
  } finally {
    if (compareCtl === ctl) comparing.value = false
  }
}

watch([pattern, sites], () => {
  if (props.mode === 'new' && !nameTouched.value && pattern.value) name.value = t('placement.name.suggest', { pattern: lt(pattern.value.name), n: sites.value })
})

watch(request, (r) => {
  if (!r) return
  clearTimeout(previewTimer)
  clearTimeout(compareTimer)
  previewTimer = setTimeout(() => void refresh(), 260)
  compareTimer = setTimeout(() => void refreshCompare(), 700)
}, { deep: true })

onMounted(async () => {
  try {
    options.value = await api.placement.options()
    const first = options.value.patterns.find((p) => p.id === 'n-sites') ?? options.value.patterns[0]
    if (first) pickPattern(first.id)
    // starting from a project: its requirements are the starting point
    if (props.mode === 'replace') {
      const r = editor.arch.requirements
      over.value = Object.fromEntries(Object.entries({ rpsPeak: r.rpsPeak, rtoSeconds: r.rtoSeconds, rpoSeconds: r.rpoSeconds, budgetMonthly: r.budgetMonthly }).filter(([, v]) => typeof v === 'number'))
    }
    if (props.mode === 'replace') name.value = editor.project?.name ?? ''
  } catch (e) {
    loadError.value = errorMessage(e)
  }
})
onBeforeUnmount(() => {
  clearTimeout(previewTimer)
  clearTimeout(compareTimer)
  previewCtl?.abort()
  compareCtl?.abort()
})

// ------------------------------------------------------------------------------------------ verdict
const report = computed(() => preview.value?.report)
const tone = computed(() => scoreTone(report.value?.score))
const objective = (id: string) => report.value?.objectives.find((o) => o.id === id)
const survives = computed(() => {
  const r = report.value
  if (!r) return []
  const out: { key: string; text: string; status: string }[] = []
  const lvl = objective('site_loss')
  if (lvl) out.push({ key: 'site_loss', text: t('placement.survives.' + (['instance', 'rack', 'geo'].includes(String(lvl.required)) ? lvl.required : 'site')), status: lvl.status })
  const many = objective('sites_loss')
  if (many && typeof many.required === 'number') out.push({ key: 'sites_loss', text: t('placement.survives.sites', { n: many.required }), status: many.status })
  return out
})
const rto = computed(() => objective('rto'))
const rpo = computed(() => objective('rpo'))
const money = computed(() => (report.value ? fmtMoney(report.value.cost.total, report.value.cost.symbol) : '—'))
const problems = computed(() => report.value?.findings ?? [])
const noData = computed(() => report.value?.counts.noData ?? 0)

const noteText = (n: PlacementPreview['notes'][number]) => {
  const key = 'placement.note.' + n.code
  const vars = { ...(n.vars ?? {}) }
  if (typeof vars.layer === 'string') vars.layer = lt(stack.value?.layers.find((l) => l.id === vars.layer)?.name) || vars.layer
  return t(key, vars)
}
const findingLine = (f: PlacementPreview['report']['findings'][number]) => findingText(app.rules.get(f.ruleId), f as unknown as Finding)
const ruleTitle = (id: string) => lt(app.rules.get(id)?.title) || id

function variantSurvives(v: Extract<PlacementVariant, { ok: true }>): string {
  const many = v.report.objectives.find((o) => o.id === 'sites_loss')
  if (many && typeof many.required === 'number') return t('placement.survives.sites', { n: many.required })
  const lvl = v.report.objectives.find((o) => o.id === 'site_loss')
  return t('placement.survives.' + (['instance', 'rack', 'geo'].includes(String(lvl?.required)) ? lvl?.required : 'site'))
}
const issuesOf = (v: Extract<PlacementVariant, { ok: true }>) => v.report.counts.critical + v.report.counts.risk

// ------------------------------------------------------------------------------------------ result
const canSubmit = computed(() => !!preview.value && !previewError.value && !computing.value && !busy.value && (props.mode === 'replace' || !!name.value.trim()))

async function submit() {
  const req = request.value
  if (!req || !canSubmit.value) return
  busy.value = true
  try {
    if (props.mode === 'new') {
      const r = await api.projects.create({ name: name.value.trim(), placement: { ...req, name: name.value.trim() }, lang: lang.value })
      emit('close')
      router.push({ name: 'editor', params: { id: r.id } })
    } else {
      if (!editor.isEmpty && !(await dialogs.confirm({ title: t('placement.apply.confirm'), confirmLabel: t('placement.apply') }))) return
      const arch = JSON.parse(JSON.stringify(preview.value!.architecture))
      arch.meta = { ...arch.meta, name: editor.arch.meta?.name ?? arch.meta?.name }
      editor.addTemplate(arch, 'replace')
      toasts.ok(t('placement.apply.done'))
      emit('close')
    }
  } catch (e) {
    toasts.error(explain(e))
  } finally {
    busy.value = false
  }
}

function setOver(key: keyof typeof over.value, e: Event) {
  const raw = (e.target as HTMLInputElement).value
  const next = { ...over.value }
  if (raw === '') delete next[key]
  else next[key] = Number(raw)
  over.value = next
}
const patternReq = (key: 'rpsPeak' | 'rtoSeconds' | 'rpoSeconds' | 'budgetMonthly') => pattern.value?.requirements[key]
</script>

<template>
  <Modal :title="t('placement.title')" size="xl" @close="emit('close')">
    <div v-if="loadError" class="banner banner-err">{{ loadError }}</div>
    <div v-else-if="!options" class="center"><span class="spinner" /></div>
    <div v-else class="wiz">
      <aside class="cfg">
        <p class="sub muted">{{ t('placement.subtitle') }}</p>

        <div v-if="mode === 'new'" class="field">
          <label>{{ t('placement.name') }}</label>
          <input v-model="name" class="input" maxlength="120" required @input="nameTouched = true" />
        </div>

        <section>
          <h4>{{ t('placement.pattern') }}</h4>
          <div class="patterns" role="radiogroup" :aria-label="t('placement.pattern')">
            <button v-for="p in options.patterns" :key="p.id" type="button" class="pat" :class="{ on: p.id === patternId }" role="radio" :aria-checked="p.id === patternId" @click="pickPattern(p.id)">
              <span class="row"><b class="grow">{{ lt(p.name) }}</b><span v-if="p.badge" class="tag tag-accent tag-mono">{{ p.badge }}</span></span>
              <span class="muted d">{{ lt(p.description) }}</span>
              <template v-if="p.id === patternId">
                <ul v-if="p.pros.length" class="pc pro"><li v-for="(x, i) in p.pros" :key="i"><Check :size="12" />{{ lt(x) }}</li></ul>
                <ul v-if="p.cons.length" class="pc con"><li v-for="(x, i) in p.cons" :key="i"><Minus :size="12" />{{ lt(x) }}</li></ul>
              </template>
            </button>
          </div>
        </section>

        <section v-if="pattern">
          <h4>{{ t('placement.params') }}</h4>
          <div class="field">
            <label>{{ t('placement.sites') }}</label>
            <div class="stepper">
              <button type="button" class="btn btn-icon" :disabled="sites <= siteRange.min" :aria-label="'−'" @click="step(-1)"><Minus :size="14" /></button>
              <input class="input mono num" type="number" :min="siteRange.min" :max="siteRange.max" :value="sites" :disabled="siteRange.min === siteRange.max" @change="setSites(Number(($event.target as HTMLInputElement).value))" />
              <button type="button" class="btn btn-icon" :disabled="sites >= siteRange.max" :aria-label="'+'" @click="step(1)"><Plus :size="14" /></button>
              <div class="quick">
                <button v-for="n in quickSites" :key="n" type="button" class="chip" :class="{ on: n === sites }" @click="setSites(n)">{{ n }}</button>
              </div>
            </div>
            <span class="hint">{{ t('placement.sites.hint') }}</span>
          </div>
          <div v-if="regionsOn" class="field">
            <label>{{ t('placement.regions') }}</label>
            <input v-model.number="regions" class="input mono num" type="number" :min="pattern.regions.min" :max="Math.min(pattern.regions.max, sites)" />
            <span class="hint">{{ t('placement.regions.hint') }}</span>
          </div>
          <div v-if="racksOn" class="field">
            <label>{{ t('placement.racks') }}</label>
            <input v-model.number="racks" class="input mono num" type="number" :min="pattern.racks.min" :max="pattern.racks.max" />
            <span class="hint">{{ t('placement.racks.hint') }}</span>
          </div>
          <div v-if="asksSites" class="field">
            <label>{{ t('placement.failures') }}</label>
            <div class="seg">
              <button type="button" class="chip" :class="{ on: failures === null }" @click="failures = null">{{ t('placement.failures.auto') }}{{ preview ? ` · ${preview.siteFailures}` : '' }}</button>
              <button v-for="k in maxFailures" :key="k" type="button" class="chip" :class="{ on: failures === k }" @click="failures = k">{{ k }}</button>
            </div>
            <span class="hint">{{ t('placement.failures.hint') }}</span>
          </div>
          <div v-if="options.siteTypes.length > 1" class="field">
            <label>{{ t('placement.siteType') }}</label>
            <select v-model="siteType" class="select"><option v-for="id in options.siteTypes" :key="id" :value="id">{{ siteTypeName(id) }}</option></select>
          </div>
        </section>

        <section v-if="stack">
          <h4>{{ t('placement.layers') }}</h4>
          <p class="hint">{{ t('placement.layers.hint') }}</p>
          <div v-for="l in optionalLayers" :key="l.id" class="layer" :class="{ stack: layerOn[l.id] && l.products.length > 1 }">
            <label class="sw"><input v-model="layerOn[l.id]" type="checkbox" /><span class="knob" /><span class="grow">{{ lt(l.name) }}</span></label>
            <span v-if="layerOn[l.id] && dcsUnused(l)" class="unused" :title="t('placement.dcs.unusedHint')">{{ t('placement.dcs.unused') }}</span>
            <div v-if="layerOn[l.id] && l.products.length > 1" class="picks">
              <select :value="kinds[l.id]" class="select input-sm" :aria-label="lt(l.name)" @change="setKind(l.id, ($event.target as HTMLSelectElement).value)">
                <option v-for="p in l.products" :key="p.kind" :value="p.kind">{{ kindName(p.kind) }}</option>
              </select>
              <select v-if="productOf(l.id)?.variants.length" :value="variants[l.id] ?? ''" class="select input-sm" :aria-label="t('product.variant')" @change="variants[l.id] = ($event.target as HTMLSelectElement).value">
                <option value="">{{ t('placement.variant.default') }}</option>
                <option v-for="v in productOf(l.id)!.variants" :key="v.id" :value="v.id">{{ v.managed ? '☁ ' : '' }}{{ variantName(kinds[l.id] ?? l.kind, v.id) }}</option>
              </select>
            </div>
          </div>
          <div v-for="l in stack.layers.filter((x) => !x.optional && x.products.length > 1)" :key="l.id" class="layer stack">
            <span class="grow lname">{{ lt(l.name) }}</span>
            <div class="picks">
              <select :value="kinds[l.id]" class="select input-sm" :aria-label="lt(l.name)" @change="setKind(l.id, ($event.target as HTMLSelectElement).value)">
                <option v-for="p in l.products" :key="p.kind" :value="p.kind">{{ kindName(p.kind) }}</option>
              </select>
              <select v-if="productOf(l.id)?.variants.length" :value="variants[l.id] ?? ''" class="select input-sm" :aria-label="t('product.variant')" @change="variants[l.id] = ($event.target as HTMLSelectElement).value">
                <option value="">{{ t('placement.variant.default') }}</option>
                <option v-for="v in productOf(l.id)!.variants" :key="v.id" :value="v.id">{{ v.managed ? '☁ ' : '' }}{{ variantName(kinds[l.id] ?? l.kind, v.id) }}</option>
              </select>
            </div>
          </div>
        </section>

        <section v-if="pattern">
          <h4>{{ t('placement.requirements') }}</h4>
          <p class="hint">{{ t('placement.requirements.hint') }}</p>
          <div class="reqs">
            <label v-for="k in (['rpsPeak', 'rtoSeconds', 'rpoSeconds', 'budgetMonthly'] as const)" :key="k" class="field">
              <span class="label">{{ t('placement.req.' + k) }}</span>
              <input class="input mono" type="number" min="0" :value="over[k] ?? ''" :placeholder="patternReq(k) === undefined || patternReq(k) === null ? '—' : String(patternReq(k))" @input="setOver(k, $event)" />
            </label>
          </div>
        </section>
      </aside>

      <section class="out" :class="{ busy: computing }" :aria-busy="computing">
        <div v-if="previewError" class="banner banner-err">{{ previewError }}</div>
        <template v-if="preview && report">
          <div class="verdict">
            <div class="score" :class="tone">
              <span class="k">{{ t('placement.verdict.score') }}</span>
              <b>{{ Math.round(report.score) }}</b><span class="of">/100</span>
              <span class="conf">{{ t('editor.score.confidence', { pct: Math.round(report.coverage * 100) }) }}</span>
            </div>
            <div class="tiles">
              <div class="tile wide">
                <span class="k">{{ t('placement.verdict.survives') }}</span>
                <span v-for="s in survives" :key="s.key" class="surv" :class="s.status"><Check v-if="s.status === 'ok'" :size="13" /><TriangleAlert v-else :size="13" />{{ s.text }}</span>
              </div>
              <div class="tile"><span class="k">RTO</span><b class="mono">{{ typeof rto?.actual === 'number' ? fmtDuration(rto.actual) : rto?.actual === 'never' ? '∞' : '—' }}</b></div>
              <div class="tile"><span class="k">RPO</span><b class="mono">{{ typeof rpo?.actual === 'number' ? (rpo.actual === 0 ? '0' : fmtDuration(rpo.actual)) : rpo?.actual === 'lost' ? '✕' : '—' }}</b></div>
              <div class="tile"><span class="k">{{ t('placement.verdict.cost') }}</span><b class="mono">{{ money }}</b></div>
              <div class="tile"><span class="k">{{ t('placement.verdict.nodes') }}</span><b class="mono small">{{ t('placement.verdict.nodes.value', { n: preview.nodes, e: preview.edges }) }}</b></div>
            </div>
          </div>
          <p v-if="report.worstCapacity !== null" class="cap muted">{{ t('placement.verdict.capacity', { pct: fmtPct(report.worstCapacity) }) }}</p>

          <h4>{{ t('placement.map') }}</h4>
          <SiteMap :architecture="preview.architecture" :sites="preview.sites" />
          <p class="legend faint">{{ t('placement.map.legend') }}</p>

          <div v-if="preview.notes.length" class="notes">
            <h4>{{ t('placement.notes') }}</h4>
            <p v-for="(n, i) in preview.notes" :key="i" class="note" :class="n.level"><Info v-if="n.level === 'info'" :size="13" /><TriangleAlert v-else :size="13" />{{ noteText(n) }}</p>
          </div>

          <h4>{{ t('placement.findings') }}</h4>
          <p v-if="!problems.length" class="ok-line"><Check :size="14" />{{ t('placement.findings.none') }}</p>
          <ul v-else class="finds">
            <li v-for="f in problems.slice(0, 6)" :key="f.id" :class="f.status">
              <b>{{ ruleTitle(f.ruleId) }}</b><span>{{ findingLine(f) }}</span>
            </li>
            <li v-if="problems.length > 6" class="more">{{ t('placement.findings.more', { n: problems.length - 6 }) }}</li>
          </ul>
          <p v-if="noData" class="muted nd">{{ t('placement.nodata') }}</p>

          <h4>{{ t('placement.compare.title') }}</h4>
          <p class="hint">{{ t('placement.compare.hint') }}</p>
          <table class="cmp">
            <thead><tr><th>{{ t('placement.compare.sites') }}</th><th>{{ t('placement.compare.survives') }}</th><th>{{ t('placement.compare.score') }}</th><th>{{ t('placement.compare.cost') }}</th><th>{{ t('placement.compare.nodes') }}</th><th>{{ t('placement.compare.problems') }}</th></tr></thead>
            <tbody>
              <template v-for="v in compare" :key="v.sites ?? 0">
                <tr v-if="v.ok" :class="{ cur: v.sites === sites }" tabindex="0" @click="v.sites && setSites(v.sites)" @keydown.enter="v.sites && setSites(v.sites)">
                  <td class="mono"><b>{{ v.workingSites }}</b><span v-if="v.arbiterSites" class="faint"> +{{ v.arbiterSites }}</span><span v-if="v.sites === sites" class="cur-tag">{{ t('placement.compare.current') }}</span></td>
                  <td>{{ variantSurvives(v) }}</td>
                  <td><span class="sc" :class="scoreTone(v.report.score)">{{ Math.round(v.report.score) }}</span></td>
                  <td class="mono">{{ fmtMoney(v.report.cost.total, v.report.cost.symbol) }}</td>
                  <td class="mono">{{ v.nodes }}</td>
                  <td><span v-if="v.report.counts.critical" class="tag tag-crit">{{ v.report.counts.critical }}</span><span v-if="v.report.counts.risk" class="tag tag-risk">{{ v.report.counts.risk }}</span><span v-if="!issuesOf(v)" class="tag tag-ok">0</span></td>
                </tr>
                <tr v-else class="bad"><td class="mono">{{ v.sites }}</td><td colspan="5" class="muted">{{ t('placement.error.' + v.error.code) }}</td></tr>
              </template>
            </tbody>
          </table>
          <p v-if="comparing && !compare.length" class="muted">{{ t('placement.compare.loading') }}</p>
        </template>
        <div v-else-if="!previewError" class="center"><span class="spinner" /> <span class="muted">{{ t('placement.loading') }}</span></div>
      </section>
    </div>

    <template #footer>
      <button class="btn" @click="emit('close')">{{ t('common.cancel') }}</button>
      <button class="btn btn-primary" :disabled="!canSubmit" @click="submit">{{ mode === 'new' ? t('placement.create') : t('placement.apply') }}</button>
    </template>
  </Modal>
</template>

<style scoped>
.center { display: flex; align-items: center; justify-content: center; gap: 10px; padding: 48px 0; }
.wiz { display: grid; grid-template-columns: minmax(300px, 380px) 1fr; gap: 22px; align-items: start; height: calc(100vh - 230px); min-height: 420px; }
.cfg, .out { height: 100%; overflow-y: auto; padding-right: 6px; scrollbar-width: thin; }
.cfg { display: flex; flex-direction: column; gap: 16px; }
.sub { margin: -4px 0 0; font-size: 13px; }
h4 { margin: 0 0 8px; font-size: 12px; text-transform: uppercase; letter-spacing: .06em; color: var(--muted); font-weight: 650; }
.out h4 { margin-top: 18px; }
section { display: flex; flex-direction: column; gap: 10px; }
.patterns { display: flex; flex-direction: column; gap: 8px; }
.pat { text-align: left; display: flex; flex-direction: column; gap: 4px; padding: 10px 12px; border: 1px solid var(--line-strong); border-radius: var(--radius); background: var(--surface); transition: border-color var(--dur-fast), background var(--dur-fast), box-shadow var(--dur-fast); }
.pat:hover { border-color: #b8c2d4; }
.pat.on { border-color: var(--accent); background: var(--accent-soft); box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 12%, transparent); }
.pat .d { font-size: 12.5px; }
.pc { list-style: none; margin: 4px 0 0; padding: 0; display: flex; flex-direction: column; gap: 2px; font-size: 12px; }
.pc li { display: flex; gap: 6px; align-items: flex-start; }
.pc svg { margin-top: 2px; flex: none; }
.pc.pro { color: var(--ok); } .pc.con { color: var(--risk); }
.stepper { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
.num { width: 76px; text-align: center; }
.quick { display: flex; gap: 4px; flex-wrap: wrap; margin-left: 4px; }
.chip { cursor: pointer; }
.chip.on { background: var(--accent-soft); border-color: var(--accent-line); color: var(--accent); font-weight: 600; }
.seg { display: flex; gap: 6px; flex-wrap: wrap; }
.layer { display: flex; align-items: center; gap: 8px; min-height: 30px; flex-wrap: wrap; }
.layer .select { width: 150px; flex: none; }
.layer.stack { padding: 4px 0 6px; border-bottom: 1px solid var(--line); }
.layer.stack:last-child { border-bottom: 0; }
.picks { display: flex; gap: 6px; flex-wrap: wrap; width: 100%; padding-left: 40px; }
.picks .select { flex: 1 1 130px; width: auto; min-width: 0; }
.unused { font-size: 11px; color: var(--muted); background: var(--surface-sunken); border-radius: 999px; padding: 1px 8px; }
.lname { font-size: 13px; }
.sw { display: flex; align-items: center; gap: 10px; cursor: pointer; flex: 1; font-size: 13px; }
.sw input { position: absolute; opacity: 0; pointer-events: none; }
.knob { width: 30px; height: 18px; border-radius: 10px; background: var(--line-strong); position: relative; transition: background var(--dur-fast); flex: none; }
.knob::after { content: ''; position: absolute; left: 2px; top: 2px; width: 14px; height: 14px; border-radius: 50%; background: #fff; box-shadow: 0 1px 2px rgba(0, 0, 0, .25); transition: transform var(--dur-fast) cubic-bezier(.2, .8, .2, 1); }
.sw input:checked + .knob { background: var(--accent); }
.sw input:checked + .knob::after { transform: translateX(12px); }
.sw input:focus-visible + .knob { outline: 2px solid var(--accent); outline-offset: 2px; }
.reqs { display: grid; grid-template-columns: 1fr 1fr; gap: 8px 10px; }
.out { min-width: 0; transition: opacity var(--dur-base); }
.out > h4:first-child { margin-top: 0; }
.out.busy { opacity: .72; }
.verdict { display: flex; gap: 14px; align-items: stretch; flex-wrap: wrap; }
.score { display: flex; flex-wrap: wrap; align-items: baseline; align-content: center; gap: 2px 4px; padding: 10px 16px; min-width: 136px; border-radius: var(--radius); border: 1px solid var(--line); background: var(--surface-2); }
.score .k, .tile .k { width: 100%; font-size: 11px; color: var(--muted); text-transform: uppercase; letter-spacing: .05em; font-weight: 600; }
.score b { font-size: 34px; letter-spacing: -0.03em; line-height: 1; }
.score .of { font-size: 12px; color: var(--muted); }
.score .conf { width: 100%; font-size: 11px; color: var(--faint); margin-top: 2px; }
.score.ok { background: var(--ok-bg); border-color: var(--ok-line); color: var(--ok); }
.score.risk { background: var(--risk-bg); border-color: var(--risk-line); color: var(--risk); }
.score.crit { background: var(--crit-bg); border-color: var(--crit-line); color: var(--crit); }
.tiles { display: flex; gap: 8px; flex-wrap: wrap; flex: 1; align-content: stretch; }
.tile { display: flex; flex-direction: column; gap: 3px; padding: 8px 12px; border: 1px solid var(--line); border-radius: var(--radius); background: var(--surface); min-width: 92px; }
.tile.wide { flex: 1 1 100%; flex-direction: row; flex-wrap: wrap; align-items: center; gap: 6px 10px; }
.tile b { font-size: 16px; }
.tile b.small { font-size: 13px; }
.surv { display: inline-flex; align-items: center; gap: 5px; font-size: 13px; font-weight: 600; padding: 2px 8px; border-radius: 10px; }
.surv.ok { color: var(--ok); background: var(--ok-bg); }
.surv.critical, .surv.risk { color: var(--crit); background: var(--crit-bg); }
.surv.no_data { color: var(--nodata); background: var(--nodata-bg); }
.cap { margin: 8px 0 0; font-size: 12.5px; }
.legend { margin: 8px 0 0; font-size: 11.5px; }
.notes .note { display: flex; gap: 8px; align-items: flex-start; font-size: 12.5px; margin: 4px 0; color: var(--text-2); }
.notes .note svg { margin-top: 2px; flex: none; color: var(--info-dot); }
.notes .note.warn svg { color: var(--risk-dot); }
.ok-line { display: flex; align-items: center; gap: 6px; color: var(--ok); font-size: 13px; margin: 0; }
.finds { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
.finds li { display: flex; flex-direction: column; gap: 1px; padding: 7px 10px; border-radius: var(--radius-sm); border: 1px solid var(--line); border-left-width: 3px; font-size: 12.5px; }
.finds li.critical { border-left-color: var(--crit); background: var(--crit-bg); }
.finds li.risk { border-left-color: var(--risk-dot); background: var(--risk-bg); }
.finds li.more { border-style: dashed; color: var(--muted); }
.nd { font-size: 12px; margin: 8px 0 0; }
.cmp { width: 100%; border-collapse: collapse; font-size: 13px; }
.cmp th { text-align: left; font-size: 11px; text-transform: uppercase; letter-spacing: .05em; color: var(--muted); font-weight: 600; padding: 4px 8px; border-bottom: 1px solid var(--line); }
.cmp td { padding: 7px 8px; border-bottom: 1px solid var(--line); }
.cmp tbody tr { cursor: pointer; transition: background var(--dur-fast); }
.cmp tbody tr:hover { background: var(--surface-2); }
.cmp tbody tr.cur { background: var(--accent-soft); }
.cmp tbody tr.bad { cursor: default; }
.cmp tbody tr:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
.cur-tag { margin-left: 8px; font: 600 10px var(--font); text-transform: uppercase; letter-spacing: .05em; color: var(--accent); }
.sc { display: inline-block; min-width: 34px; text-align: center; padding: 1px 6px; border-radius: 6px; font-weight: 700; font-size: 12.5px; background: var(--surface-sunken); }
.sc.ok { color: var(--ok); background: var(--ok-bg); } .sc.risk { color: var(--risk); background: var(--risk-bg); } .sc.crit { color: var(--crit); background: var(--crit-bg); }
.cmp .tag { margin-right: 4px; }
@media (max-width: 960px) { .wiz { grid-template-columns: 1fr; height: auto; } .cfg, .out { height: auto; overflow: visible; padding-right: 0; } }
</style>
