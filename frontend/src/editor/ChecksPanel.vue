<script setup lang="ts">
import { computed, ref } from 'vue'
import { Check, ChevronDown, ChevronRight, CircleHelp, TriangleAlert, X } from 'lucide-vue-next'
import type { Objective, Status } from '@/api/types'
import { lt, t } from '@/i18n'
import { fmtDuration, fmtPct } from '@/lib/format'
import { statusRank } from '@/lib/findings'
import { focusNeed } from '@/lib/navigate'
import { objectiveAction, type ObjectiveAction } from '@/lib/objectives'
import { scenarioLabel } from '@/lib/sim'
import { useAi } from '@/stores/ai'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'
import { useUi } from '@/stores/ui'
import AnimatedNumber from '@/components/AnimatedNumber.vue'
import { stagger } from '@/lib/motion'
import { useMotion } from '@/stores/motion'
import FindingCard from './FindingCard.vue'
import ProposalCard from './ProposalCard.vue'

const app = useApp()
const editor = useEditor()
const ai = useAi()
const ui = useUi()
const motion = useMotion()

/** Findings that appear together come one after another (and at once when nothing is animated). */
function beforeEnter(el: Element) {
  const i = Number((el as HTMLElement).dataset.i ?? 0)
  if (motion.full) (el as HTMLElement).style.transitionDelay = `${stagger(i, 40, 320) * motion.factor}ms`
}
function afterEnter(el: Element) {
  (el as HTMLElement).style.transitionDelay = ''
}

const report = computed(() => editor.report)
const counts = computed(() => report.value?.counts)
const total = computed(() => (counts.value ? counts.value.critical + counts.value.risk + counts.value.noData + counts.value.ok : 0))
const filter = ref<Status | 'all'>('all')
const showOk = ref(false)
const showAccepted = ref(false)

const open = computed(() => [...editor.openFindings].sort((a, b) => statusRank[a.status] - statusRank[b.status]).filter((f) => filter.value === 'all' || f.status === filter.value))
const accepted = computed(() => (report.value?.findings ?? []).filter((f) => f.accepted))
const okRules = computed(() => (report.value?.rules ?? []).filter((r) => r.status === 'ok'))

function pick(s: Status) {
  filter.value = filter.value === s ? 'all' : s
}

function objLabel(o: Objective): string {
  switch (o.id) {
    case 'rto': return typeof o.required === 'number' ? t('obj.rto', { v: fmtDuration(o.required) }) : t('obj.rto.none')
    case 'rpo': return typeof o.required === 'number' ? t('obj.rpo', { v: o.required === 0 ? '0' : fmtDuration(o.required) }) : t('obj.rpo.none')
    case 'site_loss': return t('obj.site_loss.' + (o.required === 'geo' ? 'geo' : o.required === 'instance' ? 'instance' : o.required === 'rack' ? 'rack' : 'site'))
    case 'sites_loss': return typeof o.required === 'number' ? t('obj.sites_loss.count', { n: o.required }) : t('obj.sites_loss')
    case 'spof': return t('obj.spof')
    case 'capacity': return t('obj.capacity')
    default: return o.id
  }
}

function objValue(o: Objective): string {
  const a = o.actual
  if (o.status === 'no_data' && (a === null || a === undefined)) return t('obj.value.nodata')
  switch (o.id) {
    case 'rto': return a === 'never' ? t('obj.value.never') : typeof a === 'number' ? fmtDuration(a) : t('obj.value.nodata')
    case 'rpo': return a === 'lost' ? t('obj.value.lost') : typeof a !== 'number' ? t('obj.value.nodata') : (a === 0 ? t('obj.value.sync') : fmtDuration(a)) + (o.assumed && a > 0 ? ' ~' : '')
    case 'site_loss':
    case 'sites_loss': return a === true ? t('obj.value.lives') : a === false ? t('obj.value.dies') : t('obj.value.nodata')
    case 'spof': return a === 0 ? t('obj.value.none') : String(a ?? '')
    case 'capacity': return typeof a === 'number' ? fmtPct(a) : t('obj.value.nodata')
    default: return String(a ?? '')
  }
}

/** A pill is a doorway to the evidence behind its value: the worst scenario, the field to fill in, the capacity plan. */
function actionOf(o: Objective): ObjectiveAction | null {
  const r = report.value
  return r ? objectiveAction(o, r, { rpsPeak: editor.arch.requirements.rpsPeak, ruleCheck: (id) => app.rules.get(id)?.check }) : null
}

function objHint(o: Objective): string {
  const a = actionOf(o)
  if (!a) return o.assumed ? t('obj.assumed') : ''
  switch (a.kind) {
    case 'scenario': return t('obj.open.scenario', { name: scenarioLabel(a.scenario, editor.shown, report.value?.units) })
    case 'fill': return t('obj.open.fill')
    case 'tab': return t('obj.open.capacity')
    case 'subjects': return t('obj.open.spof')
  }
}

function openObjective(o: Objective) {
  const a = actionOf(o)
  if (!a) return
  switch (a.kind) {
    case 'scenario': void editor.runScenario(a.scenario); break
    case 'fill': focusNeed(a.need); break
    case 'tab': ui.tab = a.tab; break
    case 'subjects': editor.showSubjects({ subjects: a.subjects }); break
  }
}

const markIcon = (s: Status) => (s === 'ok' ? Check : s === 'critical' ? X : s === 'risk' ? TriangleAlert : CircleHelp)
</script>

<template>
  <div class="checks">
    <div class="head">
      <h2 class="grow">{{ t('checks.title', { n: total }) }}</h2>
      <span class="engine mono">{{ t('checks.engine') }}</span>
    </div>

    <div v-if="editor.evalError && !report" class="banner banner-err">{{ t('editor.eval.error') }}: {{ editor.evalError }}</div>

    <div v-if="editor.isEmpty && !ai.proposal" class="empty none">{{ t('checks.emptyArch') }}</div>
    <template v-else-if="report && counts">
      <div class="tiles">
        <button class="tile crit" :class="{ on: filter === 'critical' }" @click="pick('critical')"><b><AnimatedNumber :value="counts.critical" /></b><span>{{ t('checks.critical') }}</span></button>
        <button class="tile risk" :class="{ on: filter === 'risk' }" @click="pick('risk')"><b><AnimatedNumber :value="counts.risk" /></b><span>{{ t('checks.risk') }}</span></button>
        <button class="tile nodata" :class="{ on: filter === 'no_data' }" @click="pick('no_data')"><b><AnimatedNumber :value="counts.noData" /></b><span>{{ t('checks.nodata') }}</span></button>
        <button class="tile ok" :class="{ on: showOk }" @click="showOk = !showOk"><b><AnimatedNumber :value="counts.ok" /></b><span>{{ t('checks.ok') }}</span></button>
      </div>

      <div class="objectives">
        <component :is="actionOf(o) ? 'button' : 'div'" v-for="o in report.objectives" :key="o.id" class="obj" :class="[o.status, actionOf(o) && 'act']" :title="objHint(o) || undefined" @click="openObjective(o)">
          <span class="ol truncate">{{ objLabel(o) }}</span>
          <span class="ov mono">{{ objValue(o) }}<component :is="markIcon(o.status)" :size="12" class="mark" /></span>
        </component>
      </div>
      <p class="conf" :title="t('checks.confidence.hint')">{{ t('checks.confidence', { pct: Math.round(report.score.coverage * 100) }) }}</p>
    </template>

    <div v-if="report?.issues.length" class="banner banner-warn issues">
      <b>{{ t('checks.issues') }}</b>
      <ul><li v-for="(i, k) in report.issues.slice(0, 5)" :key="k">{{ i.message }}</li></ul>
    </div>

    <div class="list">
      <ProposalCard v-if="ai.proposal" class="proposal" />
      <TransitionGroup name="list" tag="div" class="fgroup" @before-enter="beforeEnter" @after-enter="afterEnter">
        <FindingCard v-for="(f, i) in open" :key="f.id" :finding="f" :data-i="i" />
      </TransitionGroup>
      <div v-if="report && !open.length && filter === 'all' && !editor.isEmpty" class="empty none">{{ t('checks.none') }}</div>
      <div v-if="!report && editor.evaluating" class="sk-list" aria-busy="true" :aria-label="t('editor.checking')">
        <div v-for="n in 3" :key="n" class="skel sk-finding" />
      </div>

      <button v-if="accepted.length" class="fold" @click="showAccepted = !showAccepted"><component :is="showAccepted ? ChevronDown : ChevronRight" :size="14" />{{ t('checks.acceptedList', { n: accepted.length }) }}</button>
      <FindingCard v-for="f in (showAccepted ? accepted : [])" :key="f.id" :finding="f" />

      <ul v-if="showOk && okRules.length" class="oklist">
        <li v-for="r in okRules" :key="r.ruleId"><Check :size="13" class="okc" /><span class="mono rid">{{ r.ruleId }}</span><span class="grow">{{ lt(app.rules.get(r.ruleId)?.ok ?? app.rules.get(r.ruleId)?.title) }}</span></li>
      </ul>
    </div>
  </div>
</template>

<style scoped>
.checks { display: flex; flex-direction: column; min-height: 0; flex: 1; }
.head { display: flex; align-items: baseline; padding: 16px 20px 10px; }
.head h2 { font-size: 16px; }
.engine { font-size: 11.5px; color: var(--faint); }
.tiles { display: grid; grid-template-columns: repeat(4, 1fr); gap: 8px; padding: 0 20px; }
.tile { display: flex; flex-direction: column; align-items: center; gap: 1px; padding: 10px 4px 9px; border-radius: var(--radius); border: 1px solid transparent; font-size: 13px; transition: border-color var(--dur-fast), transform var(--dur-instant) var(--ease-out); }
.tile:hover { transform: translateY(-1px); }
.tile b { font-size: 24px; line-height: 1.15; font-weight: 700; letter-spacing: -0.02em; }
.tile.crit { background: var(--crit-bg); color: var(--crit); }
.tile.risk { background: var(--risk-bg); color: var(--risk); }
.tile.nodata { background: var(--nodata-bg); color: var(--nodata); }
.tile.ok { background: var(--ok-bg); color: var(--ok); }
.tile.on { border-color: currentColor; }
.objectives { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; padding: 10px 20px 0; }
.obj { display: flex; align-items: center; gap: 8px; height: 38px; padding: 0 12px; background: var(--surface); border: 1px solid var(--line-strong); border-radius: var(--radius); font-size: 12.5px; min-width: 0; transition: border-color var(--dur-base), box-shadow var(--dur-fast); }
.obj .ov, .obj .mark { transition: color var(--dur-base); }
.obj.act { cursor: pointer; text-align: left; }
.obj.act:hover { border-color: var(--accent); box-shadow: var(--shadow-1); }
.obj:last-child:nth-child(odd) { grid-column: 1 / -1; }
.ol { color: var(--muted); flex: 1; }
.ov { font-size: 12.5px; color: var(--text); white-space: nowrap; display: inline-flex; align-items: center; gap: 5px; }
.mark { flex: none; }
.obj.ok .mark { color: var(--ok); }
.obj.critical { border-color: var(--crit-line); } .obj.critical .mark, .obj.critical .ov { color: var(--crit); }
.obj.risk { border-color: var(--risk-line); } .obj.risk .mark, .obj.risk .ov { color: var(--risk); }
.obj.no_data .ov, .obj.no_data .mark { color: var(--nodata); }
.conf { margin: 8px 20px 0; font-size: 12px; color: var(--faint); }
.issues { margin: 10px 20px 0; }
.issues ul { margin: 6px 0 0; padding-left: 18px; }
.list { flex: 1; overflow: auto; padding: 14px 20px 20px; display: flex; flex-direction: column; gap: 12px; margin-top: 12px; border-top: 1px solid var(--line); background: var(--surface-2); }
.fgroup { display: flex; flex-direction: column; gap: 12px; position: relative; }
.sk-list { display: flex; flex-direction: column; gap: 12px; }
.sk-finding { height: 92px; border-radius: var(--radius); }
.none { color: var(--ok); }
.empty { display: flex; gap: 8px; align-items: center; justify-content: center; }
.fold { display: inline-flex; align-items: center; gap: 4px; border: 0; background: transparent; color: var(--muted); font-size: 12.5px; padding: 2px 0; align-self: flex-start; }
.fold:hover { color: var(--text); }
.oklist { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; font-size: 12.5px; }
.oklist li { display: flex; gap: 8px; align-items: center; color: var(--text-2); }
.okc { color: var(--ok); flex: none; }
.rid { color: var(--faint); font-size: 11.5px; }
</style>
