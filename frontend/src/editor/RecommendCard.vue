<script setup lang="ts">
import { computed, ref } from 'vue'
import { ArrowRight, Check, ChevronDown, Crosshair, ListChecks } from 'lucide-vue-next'
import type { Effect, Recommendation, RecommendPlan } from '@/api/types'
import { lt, t } from '@/i18n'
import { findingText } from '@/lib/findings'
import { effectChips, recommendationLabel } from '@/lib/recommend'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'
import { useRecommend } from '@/stores/recommend'
import { useToasts } from '@/stores/toasts'

const app = useApp()
const editor = useEditor()
const rec = useRecommend()
const toasts = useToasts()

const plan = computed<RecommendPlan | null>(() => rec.current)
const symbol = computed(() => app.boot?.engine.symbol ?? '')
const canApply = computed(() => app.canEdit && !editor.readonly)
/** The numbers on the card are for the design as it was: they wait for the new ones before anything is applied. */
const waiting = computed(() => rec.loading || !rec.fresh)
const busy = ref(false)
const all = ref(false)

const items = computed(() => plan.value?.items ?? [])
const shownItems = computed(() => (all.value ? items.value : items.value.slice(0, rec.show)))
const hidden = computed(() => Math.max(0, items.value.length - shownItems.value.length))
const bundle = computed(() => plan.value?.bundle ?? null)
const manual = computed(() => plan.value?.manual.length ?? 0)

const ruleOf = (id: string) => app.rules.get(id)
const label = (r: Recommendation) => recommendationLabel(ruleOf(r.ruleId), r)
const finding = (r: Recommendation) => editor.report?.findings.find((f) => f.id === r.findingId)
/** The problem the fix answers: the words of the finding itself, or of its rule when the report has moved on. */
const why = (r: Recommendation) => {
  const rule = ruleOf(r.ruleId)
  const f = finding(r)
  return f ? findingText(rule, f) : rule ? lt(rule.title) : r.ruleId
}
const chips = (e: Effect) => effectChips(e, symbol.value)

function show(r: Recommendation) {
  const f = finding(r)
  if (f) editor.showSubjects(f)
}

/** Apply, and say what was done with the way back: the toast carries the effect the engine measured. */
async function apply(ops: Recommendation['ops'], text: string) {
  if (busy.value || waiting.value || !canApply.value) return
  busy.value = true
  try {
    if (await editor.applyOps(ops)) toasts.undoable(text, () => editor.undo())
  } finally {
    busy.value = false
  }
}
const applyOne = (r: Recommendation) => apply(r.ops, t('rec.applied', { label: label(r) }))
const applyAll = () => bundle.value && apply(bundle.value.ops, t('rec.appliedAll', { n: bundle.value.ids.length }))
</script>

<template>
  <section v-if="rec.enabled && (plan || rec.error)" class="rec" :class="{ waiting }" :aria-busy="waiting" aria-labelledby="rec-h" data-testid="recommendations">
    <header class="rh">
      <ListChecks :size="16" class="ic" aria-hidden="true" />
      <h3 id="rec-h" class="grow">{{ t('rec.title') }}</h3>
      <span v-if="waiting && !rec.error" class="spinner" :aria-label="t('rec.updating')" role="status" />
    </header>

    <div v-if="rec.error && !plan" class="err">
      <span>{{ t('rec.error', { msg: rec.error }) }}</span>
      <button class="btn btn-sm" @click="rec.ensure()">{{ t('rec.retry') }}</button>
    </div>

    <template v-if="plan">
      <p class="hint">{{ t('rec.hint') }}</p>

      <TransitionGroup v-if="items.length" name="list" tag="ol" class="items">
        <li v-for="(r, i) in shownItems" :key="r.id" class="item" :data-rec="r.id" :data-rule="r.ruleId">
          <span class="rank" :aria-label="t('rec.rank', { n: i + 1 })">{{ i + 1 }}</span>
          <div class="body">
            <div class="head"><span class="what">{{ label(r) }}</span><span class="rid mono">{{ r.ruleId }}</span></div>
            <p class="why" :title="why(r)">{{ why(r) }}</p>
            <ul class="chips" :aria-label="t('rec.effect')">
              <li v-for="c in chips(r.effect)" :key="c.id" class="fx" :class="c.tone" :data-fx="c.id" :title="c.title">{{ c.text }}</li>
            </ul>
            <div class="acts">
              <button class="btn btn-sm btn-tonal" :disabled="!canApply || waiting || busy" :title="canApply ? undefined : t('rec.readonly')" :aria-label="t('rec.apply.aria', { label: label(r) })" @click="applyOne(r)"><Check :size="13" />{{ t('rec.apply') }}</button>
              <button v-if="finding(r)" class="btn btn-sm btn-ghost" :title="t('rec.show.hint')" @click="show(r)"><Crosshair :size="13" />{{ t('rec.show') }}</button>
            </div>
          </div>
        </li>
      </TransitionGroup>
      <p v-else class="none">{{ t('rec.none') }}</p>

      <button v-if="hidden || (all && items.length > rec.show)" class="more" @click="all = !all">
        <ChevronDown :size="14" :class="{ up: all }" />{{ all ? t('rec.less') : t('rec.more', { n: hidden }) }}
      </button>

      <div v-if="bundle" class="all" data-testid="rec-all">
        <div class="ah">
          <b class="grow">{{ t('rec.all.title', { n: bundle.ids.length }) }}</b>
          <button class="btn btn-sm btn-primary" :disabled="!canApply || waiting || busy" :title="canApply ? t('rec.all.hint') : t('rec.readonly')" @click="applyAll">{{ t('rec.all.apply', { n: bundle.ids.length }) }}<ArrowRight :size="13" /></button>
        </div>
        <ul class="chips" :aria-label="t('rec.effect')">
          <li v-for="c in chips(bundle.effect)" :key="c.id" class="fx" :class="c.tone" :data-fx="c.id" :title="c.title">{{ c.text }}</li>
        </ul>
      </div>

      <p v-if="manual" class="note" data-testid="rec-manual">{{ t('rec.manual', { n: manual }) }}</p>
      <p v-if="plan.truncated" class="note faint">{{ t('rec.truncated', { n: plan.considered }) }}</p>
    </template>
  </section>
</template>

<style scoped>
.rec { background: var(--surface); border: 1px solid var(--accent-line); border-radius: var(--radius-lg); padding: 14px 16px 14px; box-shadow: var(--shadow-1); display: flex; flex-direction: column; gap: 10px; }
.rh { display: flex; align-items: center; gap: 8px; }
.rh h3 { margin: 0; font-size: 14.5px; }
.ic { color: var(--accent); flex: none; }
.hint { margin: -4px 0 0; font-size: 12.5px; color: var(--muted); line-height: 1.45; }
.err { display: flex; align-items: center; justify-content: space-between; gap: 10px; font-size: 12.5px; color: var(--crit); }
.items { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 4px; transition: opacity var(--dur-base); }
.rec.waiting .items, .rec.waiting .all { opacity: .55; }
.item { display: flex; gap: 10px; padding: 10px 0; border-top: 1px solid var(--line); }
.item:first-child { border-top: 0; padding-top: 4px; }
.rank { flex: none; width: 22px; height: 22px; border-radius: 50%; background: var(--accent-soft); color: var(--accent); font-size: 12px; font-weight: 700; display: inline-flex; align-items: center; justify-content: center; margin-top: 1px; }
.body { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 5px; }
.head { display: flex; align-items: flex-start; gap: 8px; }
.what { flex: 1; min-width: 0; font-size: 13.5px; font-weight: 600; line-height: 1.35; color: var(--text); }
.rid { flex: none; color: var(--faint); font-size: 11.5px; letter-spacing: .02em; line-height: 1.5; }
.why { margin: 0; font-size: 12.5px; line-height: 1.45; color: var(--muted); display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; overflow: hidden; }
.chips { list-style: none; margin: 0; padding: 0; display: flex; flex-wrap: wrap; gap: 5px; }
.fx { display: inline-flex; align-items: center; height: 22px; padding: 0 8px; border-radius: 11px; font-size: 11.5px; font-weight: 550; white-space: nowrap; font-variant-numeric: tabular-nums; background: var(--surface-sunken); color: var(--text-2); }
.fx.good { background: var(--ok-bg); color: var(--ok); }
.fx.bad { background: var(--crit-bg); color: var(--crit); }
.acts { display: flex; align-items: center; gap: 6px; margin-top: 2px; }
.more { align-self: flex-start; display: inline-flex; align-items: center; gap: 4px; border: 0; background: transparent; color: var(--muted); font-size: 12.5px; padding: 2px 0; }
.more:hover { color: var(--text); }
.more svg { transition: transform var(--dur-base) var(--ease-out); }
.more svg.up { transform: rotate(180deg); }
.all { display: flex; flex-direction: column; gap: 8px; padding: 10px 12px 12px; background: var(--accent-soft); border: 1px solid var(--accent-line); border-radius: var(--radius); transition: opacity var(--dur-base); }
.ah { display: flex; align-items: center; gap: 10px; font-size: 13px; }
.all .fx { background: var(--surface); }
.all .fx.good { background: var(--ok-bg); }
.all .fx.bad { background: var(--crit-bg); }
.all .btn { flex: none; }
.none { margin: 0; font-size: 12.5px; color: var(--muted); }
.note { margin: 0; font-size: 12.5px; color: var(--text-2); line-height: 1.45; }
.note.faint { color: var(--faint); }
</style>
