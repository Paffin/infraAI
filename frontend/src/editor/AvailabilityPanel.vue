<script setup lang="ts">
import { computed } from 'vue'
import { useRouter } from 'vue-router'
import { Crosshair, Play } from 'lucide-vue-next'
import AnimatedNumber from '@/components/AnimatedNumber.vue'
import type { AvailabilityFailure } from '@/api/types'
import { t } from '@/i18n'
import { CAUSE_ORDER, allowedText, budgetUsed, causeLabel, downtimeText, rateText, standing } from '@/lib/availability'
import { formatAvailability, targetFraction } from '@/lib/nines'
import { scenarioLabel } from '@/lib/sim'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'

const app = useApp()
const editor = useEditor()
const router = useRouter()

const report = computed(() => editor.report)
const av = computed(() => report.value?.availability ?? null)
const req = computed(() => editor.arch.requirements)
const enabled = computed(() => app.boot?.engine.availability?.enabled !== false)
const presets = computed(() => app.boot?.engine.availability?.presets ?? av.value?.basis.presets ?? [])
const days = computed(() => av.value?.basis.daysPerYear ?? app.boot?.engine.availability?.daysPerYear ?? 365.25)
const canPick = computed(() => app.canEdit && !editor.readonly)

const state = computed(() => (av.value ? standing(av.value) : 'none'))
const used = computed(() => (av.value ? budgetUsed(av.value) : null))
const usedPct = computed(() => (used.value === null || !Number.isFinite(used.value) ? 100 : Math.round(used.value * 100)))
const warnPct = computed(() => av.value?.basis.warnPercent ?? 80)
const tone = computed(() => (state.value === 'over' ? 'crit' : state.value === 'close' ? 'risk' : state.value === 'within' ? 'ok' : 'nodata'))

/** The target the requirement holds, as a percentage for the chips. */
const targetPct = computed(() => (req.value.availabilitySlo === undefined || req.value.availabilitySlo === null ? null : targetFraction(req.value.availabilitySlo) * 100))
const isTarget = (p: number) => targetPct.value !== null && Math.abs(targetPct.value - p) < 1e-9
function pick(p: number) {
  editor.setRequirement('availabilitySlo', isTarget(p) ? undefined : p)
}

const causes = computed(() => {
  const list = av.value?.causes ?? []
  const max = Math.max(...list.map((c) => c.secondsPerYear), 1)
  return [...list].sort((a, b) => b.secondsPerYear - a.secondsPerYear || CAUSE_ORDER.indexOf(a.cause) - CAUSE_ORDER.indexOf(b.cause)).map((c) => ({ ...c, width: (c.secondsPerYear / max) * 100 }))
})
const units = computed(() => {
  const list = av.value?.units ?? []
  const max = Math.max(...list.map((u) => u.secondsPerYear), 1)
  return list.map((u) => ({ ...u, width: (u.secondsPerYear / max) * 100 }))
})

/** The cards that make up a service: a node of that id or the members of a group of that name. */
function showUnit(key: string) {
  const nodes = editor.arch.nodes.filter((n) => (n.group || n.id) === key).map((n) => n.id)
  if (nodes.length) editor.showSubjects({ subjects: { nodes, edges: [], zones: [] } })
}
function replay(f: AvailabilityFailure) {
  void editor.runScenario(f.scenario)
}
const label = (f: AvailabilityFailure) => scenarioLabel(f.scenario, editor.shown, report.value?.units)

const downNames = computed(() => (av.value?.units ?? []).map((u) => u.name).join(', ') || '—')
const pct = (n: number) => (n >= 0.1 ? Math.round(n * 100) : Math.round(n * 1000) / 10) + '%'

// what the estimate rests on, in the words of the requirement they answer
const basis = computed(() => av.value?.basis ?? null)
const dcRate = computed(() => basis.value?.failures.zone.dc ?? basis.value?.failures.zone.site)
const dcRepair = computed(() => basis.value?.repair.zone.dc ?? basis.value?.repair.zone.site)
function toSettings() {
  void router.push('/admin')
}
</script>

<template>
  <div class="av">
    <div class="head">
      <h2 class="grow">{{ t('avail.title') }}</h2>
    </div>

    <div v-if="editor.isEmpty" class="empty">{{ t('avail.empty.arch') }}</div>
    <div v-else-if="!enabled && !av" class="empty">{{ t('avail.off') }}</div>
    <div v-else-if="!av" class="empty">{{ report ? t('avail.noestimate') : '' }}<span v-if="!report" class="spinner" /></div>

    <template v-else>
      <p class="intro">{{ t('avail.intro') }}</p>

      <section class="hero" :class="tone" data-testid="avail-hero">
        <div class="big">
          <AnimatedNumber class="num" :value="av.estimate * 100" :decimals="4" :format="(n) => formatAvailability(n / 100)" />
          <span class="cap">{{ t('avail.estimate') }}</span>
        </div>
        <div class="side">
          <span class="tgt mono">{{ av.target !== null ? t('avail.target', { v: formatAvailability(av.target) }) : t('avail.target.none') }}</span>
          <span class="dt">{{ t('avail.down', { v: downtimeText(av.downtimeSecondsPerYear) }) }}</span>
          <span v-if="av.budgetSecondsPerYear !== null" class="bd faint">{{ t('avail.budget', { v: downtimeText(av.budgetSecondsPerYear) }) }}</span>
        </div>
      </section>

      <div v-if="av.budgetSecondsPerYear !== null" class="gauge" :class="tone">
        <div class="track" role="img" :aria-label="t('avail.budget.used', { pct: usedPct })">
          <i class="fill bar-fill" :style="{ width: Math.min(100, usedPct) + '%' }" />
          <i class="tick" :style="{ left: Math.min(100, warnPct) + '%' }" :title="t('avail.standing.close')" />
        </div>
        <div class="gl">
          <span class="verdict">{{ t('avail.standing.' + state) }}</span>
          <span class="used faint">{{ t('avail.budget.used', { pct: usedPct }) }}</span>
        </div>
      </div>
      <p v-else class="gl alone verdict nodata">{{ t('avail.standing.none') }}</p>

      <p v-if="av.serviceDown" class="note crit">{{ t('avail.servicedown', { names: downNames }) }}</p>
      <p v-if="av.truncated" class="note risk">{{ t('avail.truncated') }}</p>

      <section v-if="presets.length" class="pick" :aria-label="t('avail.pick')">
        <div class="pl"><b>{{ t('avail.pick') }}</b><span class="faint">{{ t('avail.pick.hint') }}</span></div>
        <div class="chips">
          <button v-for="p in presets" :key="p" class="chip pr" :class="{ on: isTarget(p) }" :disabled="!canPick" :aria-pressed="isTarget(p)" @click="pick(p)">
            <b class="mono">{{ formatAvailability(p / 100) }}</b>
            <span>{{ allowedText(p / 100, days) }}</span>
          </button>
        </div>
      </section>

      <template v-if="!av.serviceDown">
        <h3>{{ t('avail.causes') }}</h3>
        <ul class="rows causes">
          <li v-for="c in causes" :key="c.cause" class="line" :data-cause="c.cause">
            <div class="rl"><span class="rn">{{ causeLabel(c.cause) }}</span><span class="grow" /><span class="mono">{{ downtimeText(c.secondsPerYear) }}</span><span class="mono faint sh">{{ pct(c.share) }}</span></div>
            <div class="bar"><i class="fill bar-fill" :style="{ width: Math.max(2, c.width) + '%' }" /></div>
          </li>
        </ul>

        <h3>{{ t('avail.units') }}</h3>
        <ul class="rows units">
          <li v-for="u in units" :key="u.unit" class="line">
            <div class="rl">
              <button class="name" :title="t('avail.units.show')" @click="showUnit(u.unit)"><Crosshair :size="13" /><b class="truncate">{{ u.name }}</b></button>
              <span class="grow" /><span class="mono">{{ downtimeText(u.secondsPerYear) }}</span><span class="mono faint sh">{{ pct(u.share) }}</span>
            </div>
            <div class="bar"><i class="fill bar-fill" :style="{ width: Math.max(2, u.width) + '%' }" /></div>
          </li>
        </ul>

        <h3>{{ t('avail.top') }}</h3>
        <ul class="rows top">
          <li v-for="(f, i) in av.top" :key="i" class="fail">
            <div class="fh">
              <b class="fname truncate" :title="label(f)">{{ label(f) }}</b>
              <b class="mono cost" :title="t('avail.top.cost')">{{ downtimeText(f.secondsPerYear) }}</b>
              <button class="btn btn-ghost btn-icon" :title="t('avail.top.replay')" :aria-label="t('avail.top.replay')" @click="replay(f)"><Play :size="13" /></button>
            </div>
            <div class="fb">
              <span class="rate">{{ t('avail.top.line', { rate: rateText(f.perYear), dur: downtimeText(f.downtimeSeconds) }) }}</span>
              <span class="tag cause" :class="'cause-' + f.cause">{{ causeLabel(f.cause) }}</span>
              <span class="tag basis" :title="t('avail.basis.' + f.basis + '.hint')">{{ t('avail.basis.' + f.basis) }}</span>
            </div>
          </li>
        </ul>
      </template>

      <details v-if="basis" class="assume">
        <summary>{{ t('avail.assume') }}</summary>
        <p>{{ t('avail.assume.intro') }}</p>
        <dl>
          <dt>{{ t('avail.assume.instance') }}</dt><dd class="mono">{{ t('avail.assume.each', { rate: rateText(basis.failures.instance), dur: downtimeText(basis.repair.instance) }) }}</dd>
          <dt>{{ t('avail.assume.unit') }}</dt><dd class="mono">{{ t('avail.assume.each', { rate: rateText(basis.failures.unit), dur: downtimeText(basis.repair.unit) }) }}</dd>
          <template v-if="dcRate"><dt>{{ t('avail.assume.dc') }}</dt><dd class="mono">{{ t('avail.assume.each', { rate: rateText(dcRate), dur: downtimeText(dcRepair ?? 0) }) }}</dd></template>
          <dt>{{ t('avail.assume.link') }}</dt><dd class="mono">{{ t('avail.assume.each', { rate: rateText(basis.failures.link), dur: downtimeText(basis.repair.link) }) }}</dd>
          <dt>{{ t('avail.assume.geo') }}</dt><dd class="mono">{{ t('avail.assume.each', { rate: rateText(basis.failures.geo), dur: downtimeText(basis.repair.geo) }) }}</dd>
          <dt>{{ t('avail.assume.provider') }}</dt><dd class="mono">{{ t('avail.assume.provider.value', { sla: formatAvailability(basis.provider.default), inc: downtimeText(basis.provider.incidentSeconds) }) }}</dd>
        </dl>
        <p v-if="basis.partialCapacity" class="faint">{{ t('avail.assume.partial') }}</p>
        <button v-if="app.isAdmin" class="btn btn-sm" @click="toSettings">{{ t('avail.assume.admin') }}</button>
      </details>

      <p class="foot faint">{{ t('avail.method') }}</p>
    </template>
  </div>
</template>

<style scoped>
.av { display: flex; flex-direction: column; min-height: 0; flex: 1; overflow: auto; padding-bottom: 20px; }
.head { display: flex; align-items: baseline; padding: 16px 20px 4px; }
.head h2 { font-size: 16px; }
.intro { margin: 0; padding: 0 20px 10px; font-size: 12.5px; color: var(--muted); line-height: 1.5; }
.empty { padding: 24px 20px; color: var(--muted); display: flex; gap: 8px; align-items: center; justify-content: center; text-align: center; }
h3 { margin: 18px 20px 8px; font-size: 13px; font-weight: 600; color: var(--text-2); }

.hero { margin: 0 20px; padding: 14px 16px; border-radius: var(--radius-lg); border: 1px solid transparent; display: flex; align-items: flex-end; justify-content: space-between; gap: 12px; }
.hero.ok { background: var(--ok-bg); color: var(--ok); }
.hero.risk { background: var(--risk-bg); color: var(--risk); }
.hero.crit { background: var(--crit-bg); color: var(--crit); }
.hero.nodata { background: var(--nodata-bg); color: var(--text-2); }
.big { display: flex; flex-direction: column; min-width: 0; }
.big .num { font-size: 34px; line-height: 1.1; font-weight: 700; letter-spacing: -0.03em; }
.big .cap { font-size: 12px; opacity: .85; }
.side { display: flex; flex-direction: column; align-items: flex-end; gap: 1px; font-size: 12.5px; text-align: right; }
.side .tgt { font-size: 13px; font-weight: 600; }
.side .dt, .side .bd { color: var(--text-2); }

.gauge { margin: 10px 20px 0; display: flex; flex-direction: column; gap: 6px; }
.track { position: relative; height: 8px; border-radius: 4px; background: var(--surface-sunken); }
.track .fill { position: absolute; inset: 0 auto 0 0; border-radius: 4px; background: var(--ok-dot); }
.gauge.risk .fill { background: var(--risk-dot); }
.gauge.crit .fill { background: var(--crit); }
.track .tick { position: absolute; top: -3px; bottom: -3px; width: 2px; margin-left: -1px; background: var(--ink); opacity: .45; border-radius: 1px; }
.gl { display: flex; justify-content: space-between; align-items: baseline; gap: 12px; font-size: 12.5px; }
.gl .used { text-align: right; }
.gl.alone { margin: 10px 20px 0; }
.verdict { font-weight: 600; }
.gl .verdict { flex: none; }
.gauge.ok .verdict { color: var(--ok); } .gauge.risk .verdict { color: var(--risk); } .gauge.crit .verdict { color: var(--crit); }
.verdict.nodata { color: var(--nodata); }
.note { margin: 10px 20px 0; padding: 8px 12px; border-radius: var(--radius-sm); font-size: 12.5px; line-height: 1.45; }
.note.crit { background: var(--crit-bg); color: var(--crit); }
.note.risk { background: var(--risk-bg); color: var(--risk); }

.pick { margin: 14px 20px 0; padding: 12px 14px; background: var(--surface); border: 1px solid var(--line-strong); border-radius: var(--radius); display: flex; flex-direction: column; gap: 8px; }
.pl { display: flex; align-items: baseline; justify-content: space-between; gap: 8px; font-size: 13px; }
.pl .faint { font-size: 12px; }
.chips { display: grid; grid-template-columns: repeat(auto-fill, minmax(104px, 1fr)); gap: 6px; }
.chip.pr { display: flex; flex-direction: column; align-items: flex-start; gap: 0; height: auto; padding: 6px 10px; text-align: left; }
.chip.pr b { font-size: 13px; }
.chip.pr span { font-size: 11.5px; color: var(--muted); white-space: nowrap; }
.chip.pr.on { background: var(--accent-soft); border-color: var(--accent-line); color: var(--accent); }
.chip.pr.on span { color: var(--accent); }
.chip.pr:disabled { opacity: .55; cursor: not-allowed; }

.rows { list-style: none; margin: 0; padding: 0 20px; display: flex; flex-direction: column; gap: 9px; }
.line { display: flex; flex-direction: column; align-items: stretch; gap: 4px; }
.rl { display: flex; align-items: center; gap: 8px; font-size: 13px; min-width: 0; }
.rl .sh { min-width: 38px; text-align: right; font-size: 12px; }
.rn { color: var(--text); }
.bar { height: 6px; border-radius: 3px; background: var(--surface-sunken); overflow: hidden; }
.bar .fill { display: block; height: 100%; border-radius: 3px; background: var(--accent); }
.causes .bar .fill { background: var(--accent); }
.units .bar .fill { background: var(--info-dot, var(--accent)); }
.name { display: inline-flex; align-items: center; gap: 6px; min-width: 0; border: 0; background: transparent; padding: 2px 4px 2px 0; border-radius: 4px; color: var(--text); font-size: 13px; }
.name svg { color: var(--faint); flex: none; }
.name:hover svg { color: var(--accent); }

.top { gap: 8px; }
.fail { background: var(--surface); border: 1px solid var(--line-strong); border-radius: var(--radius); padding: 9px 12px; display: flex; flex-direction: column; gap: 6px; }
.fh { display: flex; align-items: center; gap: 8px; min-width: 0; }
.fname { font-size: 13px; min-width: 0; flex: 1; }
.fh .cost { flex: none; font-size: 13px; }
.fh .btn { flex: none; width: 26px; height: 26px; margin: -2px -4px -2px 0; }
.fb { display: flex; align-items: center; flex-wrap: wrap; gap: 6px 8px; font-size: 12.5px; color: var(--text-2); }
.fb .rate { margin-right: 2px; }
.fb .tag { text-transform: none; letter-spacing: 0; font-weight: 500; }
.tag.cause { background: var(--accent-soft); color: var(--accent); border-color: transparent; }
.tag.basis { background: var(--surface-sunken); color: var(--text-2); border-color: transparent; }

.assume { margin: 16px 20px 0; padding: 10px 14px; background: var(--surface-2); border: 1px solid var(--line); border-radius: var(--radius); font-size: 12.5px; }
.assume summary { cursor: pointer; font-weight: 600; font-size: 13px; }
.assume p { margin: 8px 0; line-height: 1.5; color: var(--muted); }
.assume dl { display: grid; grid-template-columns: 1fr auto; gap: 4px 12px; margin: 8px 0; }
.assume dt { color: var(--text-2); }
.assume dd { margin: 0; text-align: right; font-size: 12px; }
.foot { margin: 0; padding: 12px 20px 0; font-size: 12px; line-height: 1.5; }
</style>
