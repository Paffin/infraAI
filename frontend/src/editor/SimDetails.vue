<script setup lang="ts">
import { computed } from 'vue'
import { ChevronLeft, ChevronRight, Pause, Play, RotateCcw, X } from 'lucide-vue-next'
import AnimatedNumber from '@/components/AnimatedNumber.vue'
import { t } from '@/i18n'
import { fmtDuration, fmtNumber } from '@/lib/format'
import { stepAtPercent, trackPosition } from '@/lib/playback'
import { eventText, scenarioLabel } from '@/lib/sim'
import { amountText, detoursOf, lossesOf, missesOf } from '@/lib/traffic'
import { useEditor } from '@/stores/editor'
import { PLAYBACK_SPEEDS, usePlayback } from '@/stores/playback'

const editor = useEditor()
const playback = usePlayback()
const sim = computed(() => editor.sim)
const res = computed(() => sim.value?.result)
const title = computed(() => (sim.value ? scenarioLabel(sim.value.scenario, editor.arch, editor.report?.units) : ''))

const steps = computed(() => playback.steps)
const count = computed(() => steps.value.length)
const progress = computed(() => trackPosition(playback.index, count.value))
/** The service state of the step on screen: what the diagram shows is not the settled state until the last step. */
const shownState = computed(() => playback.current?.state ?? res.value?.state ?? 'up')
const stateText = computed(() => (playback.beforeFailure ? t('sim.before.state') : t('sim.result.' + shownState.value)))
const clockText = computed(() => (playback.beforeFailure ? t('play.before') : playback.clock < 0.5 ? t('play.now') : '+' + fmtDuration(Math.round(playback.clock))))
const atText = (s: { at: number; kind: string }) => (s.kind === 'before' ? t('play.before') : s.at <= 0 ? t('play.now') : '+' + fmtDuration(s.at))

// ---- the traffic of the picture on screen
const traffic = computed(() => playback.traffic)
const rps = computed(() => editor.arch.requirements.rpsPeak ?? null)
const pct = (v: number) => Math.round(v * 100)
const servedPct = computed(() => (traffic.value ? pct(traffic.value.served) : null))
const normalPct = computed(() => (playback.before ? pct(playback.before.served) : null))
const servedRps = computed(() => (traffic.value && rps.value ? Math.round(traffic.value.served * rps.value) : null))
const amount = (v: number) => {
  const a = amountText(v, rps.value, fmtNumber)
  return a.unit === 'rps' ? `${a.value} ${t('traffic.unit.rps')}` : `${a.value}%`
}
const nameOf = (id: string) => editor.nodeById.get(id)?.name ?? id
const ends = (edgeId: string) => {
  const e = editor.edgeById.get(edgeId)
  return { from: e ? nameOf(e.source) : edgeId, to: e ? nameOf(e.target) : '' }
}
const SHOWN = 2
const losses = computed(() => lossesOf(traffic.value))
const lossLines = computed(() => losses.value.slice(0, SHOWN).map((l) => t('traffic.lost', { ...ends(l.edge), amount: amount(l.amount) })))
const misses = computed(() => missesOf(traffic.value))
const missLines = computed(() => misses.value.slice(0, SHOWN).map((l) => t('traffic.missed', { ...ends(l.edge), amount: amount(l.amount) })))
const detours = computed(() => detoursOf(traffic.value))
const detourLines = computed(() => detours.value.slice(0, SHOWN).map((d) => t('traffic.detour', { from: nameOf(d.from), to: nameOf(d.to), amount: amount(d.flow + (d.lost ?? 0)) })))
const lostShare = computed(() => (servedPct.value === null ? 0 : 100 - servedPct.value))
const barTone = computed(() => (servedPct.value === null ? '' : servedPct.value >= 100 ? 'ok' : servedPct.value > 0 ? 'risk' : 'crit'))

function onTrack(e: MouseEvent) {
  const el = e.currentTarget as HTMLElement
  const r = el.getBoundingClientRect()
  playback.seek(stepAtPercent(((e.clientX - r.left) / Math.max(1, r.width)) * 100, count.value))
}
function onKey(e: KeyboardEvent) {
  if (e.key === 'ArrowRight' || e.key === 'ArrowUp') { e.preventDefault(); playback.step(1) }
  else if (e.key === 'ArrowLeft' || e.key === 'ArrowDown') { e.preventDefault(); playback.step(-1) }
  else if (e.key === 'Home') { e.preventDefault(); playback.seek(0) }
  else if (e.key === 'End') { e.preventDefault(); playback.seek(playback.last) }
}
</script>

<template>
  <div v-if="sim && !res" class="simcard waiting" role="region" :aria-label="t('tab.sim')" data-testid="simcard">
    <p v-if="sim.error" class="banner banner-err">{{ sim.error }}</p>
    <p v-else class="wait"><span class="spinner" aria-hidden="true" />{{ t('sim.running') }}</p>
    <button class="btn btn-sm" @click="editor.exitSim()">{{ t('sim.exit') }}</button>
  </div>
  <div v-else-if="sim && res" class="simcard" role="region" :aria-label="t('tab.sim')" data-testid="simcard">
    <header>
      <span class="dot" /><h2 class="grow truncate">{{ t('sim.banner', { failed: res.failed.length ? res.failed.join(', ') : title }) }}</h2>
      <button class="btn btn-ghost btn-icon" :aria-label="t('sim.exit')" :title="t('sim.exit')" @click="editor.exitSim()"><X :size="16" /></button>
    </header>
    <p class="state" :class="playback.beforeFailure ? 'up' : shownState" data-testid="sim-state">{{ stateText }}<span v-if="!playback.available && res.downUnits.length"> · {{ res.downUnits.join(', ') }}</span></p>

    <section v-if="playback.available" class="player" :aria-label="t('play.title')">
      <div class="ctl">
        <button class="pbtn main" :aria-label="playback.playing ? t('play.pause') : t('play.play')" :title="playback.playing ? t('play.pause') : t('play.play')" data-testid="play" @click="playback.toggle()">
          <component :is="playback.playing ? Pause : Play" :size="15" />
        </button>
        <button class="pbtn" :aria-label="t('play.prev')" :title="t('play.prev')" :disabled="playback.index === 0" @click="playback.step(-1)"><ChevronLeft :size="15" /></button>
        <button class="pbtn" :aria-label="t('play.next')" :title="t('play.next')" :disabled="playback.index >= playback.last" @click="playback.step(1)"><ChevronRight :size="15" /></button>
        <button class="pbtn" :aria-label="t('play.restart')" :title="t('play.restart')" :disabled="playback.index === 0" @click="playback.seek(0)"><RotateCcw :size="14" /></button>
        <span class="clock mono" data-testid="clock">{{ clockText }}</span>
        <span class="grow" />
        <span class="speeds" role="group" :aria-label="t('play.speed')">
          <button v-for="s in PLAYBACK_SPEEDS" :key="s" class="sp" :class="{ on: playback.speed === s }" @click="playback.setSpeed(s)">×{{ s }}</button>
        </span>
      </div>
      <div class="track" role="slider" tabindex="0" :aria-label="t('play.title')" :aria-valuemin="1" :aria-valuemax="count" :aria-valuenow="playback.index + 1" :aria-valuetext="t('play.step', { i: playback.index + 1, n: count })" @click="onTrack" @keydown="onKey">
        <i class="rail" /><i class="fill" :style="{ width: progress + '%' }" />
        <span v-for="s in steps" :key="s.index" class="tick" :class="[s.state, { on: s.index <= playback.index, cur: s.index === playback.index }]" :style="{ left: trackPosition(s.index, count) + '%' }" :title="atText(s)" aria-hidden="true" />
      </div>
    </section>

    <section v-if="traffic" class="traffic" data-testid="traffic" :aria-label="t('traffic.title')">
      <h3>{{ t('traffic.title') }}</h3>
      <div class="big">
        <b class="num" :class="barTone" data-testid="traffic-served"><AnimatedNumber :value="servedPct" :format="(n) => n + '%'" /></b>
        <span class="what">{{ t('traffic.served') }}</span>
        <span v-if="normalPct !== null && !playback.beforeFailure && normalPct !== servedPct" class="was">{{ t('traffic.was', { pct: normalPct + '%' }) }}</span>
      </div>
      <p v-if="servedRps !== null && rps" class="rps">{{ fmtNumber(servedRps) }} {{ t('traffic.of', { total: fmtNumber(rps) }) }}</p>
      <div class="bar" :class="barTone" role="img" :aria-label="`${servedPct}%`"><i class="ok" :style="{ width: servedPct + '%' }" /><i class="gone" :style="{ width: lostShare + '%' }" /></div>
      <div class="legend" aria-hidden="true">
        <span class="lg flow"><i />{{ t('traffic.legend.flow') }}</span>
        <span class="lg lost"><i />{{ t('traffic.legend.lost') }}</span>
        <span v-if="misses.length" class="lg missed"><i />{{ t('traffic.legend.missed') }}</span>
        <span class="lg detour"><i />{{ t('traffic.legend.detour') }}</span>
      </div>
      <ul v-if="lossLines.length || missLines.length || detourLines.length" class="notes">
        <li v-for="(l, i) in lossLines" :key="'l' + i" class="lost" data-testid="traffic-lost">{{ l }}</li>
        <li v-if="losses.length > SHOWN" class="more">{{ t('traffic.lost.more', { n: losses.length - SHOWN }) }}</li>
        <li v-for="(l, i) in missLines" :key="'m' + i" class="missed" data-testid="traffic-missed">{{ l }}</li>
        <li v-if="misses.length > SHOWN" class="more">{{ t('traffic.missed.more', { n: misses.length - SHOWN }) }}</li>
        <li v-for="(l, i) in detourLines" :key="'d' + i" class="detour" data-testid="traffic-detour">{{ l }}</li>
        <li v-if="detours.length > SHOWN" class="more">{{ t('traffic.detour.more', { n: detours.length - SHOWN }) }}</li>
      </ul>
      <p v-else-if="!playback.beforeFailure" class="muted small">{{ t('traffic.nolost') }}</p>
    </section>

    <div class="scroll">
    <h3>{{ t('sim.details') }}</h3>
    <ol v-if="playback.available" class="steps">
      <li v-for="s in steps" :key="s.index" :class="[{ cur: s.index === playback.index, past: s.index < playback.index, future: s.index > playback.index }, s.kind]" :data-step="s.index" @click="playback.seek(s.index)">
        <span class="at mono">{{ atText(s) }}</span>
        <ul>
          <template v-if="s.kind === 'before'">
            <li><b>{{ t('play.before.step') }}</b></li>
            <li class="muted">{{ t('play.before.text') }}</li>
          </template>
          <template v-else>
            <li v-for="(e, i) in s.events" :key="i">{{ eventText(e) }}</li>
            <li v-if="!s.events.length" class="muted">{{ t('play.settled') }}</li>
          </template>
        </ul>
        <span v-if="s.served !== null" class="served" :class="s.served >= 0.999 ? 'ok' : s.served > 0.001 ? 'risk' : 'crit'" :title="t('traffic.served')">{{ pct(s.served) }}%</span>
      </li>
    </ol>
    <ul v-else>
      <li v-for="(e, i) in res.events" :key="i">{{ eventText(e) }}</li>
      <li v-if="!res.events.length" class="muted">{{ t('sim.noevents') }}</li>
    </ul>
    <p v-if="res.rpoAssumed" class="muted small">{{ t('sim.assumed') }}</p>
    </div>
  </div>
</template>

<style scoped>
.simcard { display: flex; flex-direction: column; min-height: 0; flex: 1; padding: 14px 20px 0; }
header { display: flex; align-items: center; gap: 9px; margin: 0 -8px 2px 0; }
header h2 { font-size: 16px; margin: 0; }
.dot { width: 9px; height: 9px; border-radius: 50%; background: var(--crit); flex: none; box-shadow: 0 0 0 3px var(--crit-bg); }
.state { margin: 0 0 10px; font-weight: 600; font-size: 13px; transition: color var(--dur-base); }
.state.up { color: var(--ok); } .state.degraded { color: var(--risk); } .state.down { color: var(--crit); }
h3 { margin: 10px 0 6px; font-size: 10.5px; letter-spacing: .09em; text-transform: uppercase; color: var(--label); font-weight: 600; }
ul { margin: 0; padding-left: 16px; display: flex; flex-direction: column; gap: 6px; font-size: 12.5px; line-height: 1.45; color: var(--text-2); }
.small { font-size: 12px; margin: 8px 0 0; }
.waiting { gap: 14px; align-items: flex-start; }
.wait { display: flex; align-items: center; gap: 10px; margin: 6px 0 0; color: var(--muted); font-size: 13px; }
.scroll { overflow: auto; min-height: 0; flex: 1 1 auto; margin: 0 -20px; padding: 0 20px 18px; overscroll-behavior: contain; }
.simcard > header, .simcard > .state, .simcard > .player, .simcard > .traffic { flex: none; }

.player { background: var(--ink); color: var(--on-dark); border-radius: 10px; padding: 8px 10px 12px; margin: 2px 0 4px; }
.ctl { display: flex; align-items: center; gap: 4px; }
.pbtn { width: 28px; height: 28px; display: inline-flex; align-items: center; justify-content: center; border: 1px solid var(--line-on-dark-faint); border-radius: 7px; background: transparent; color: var(--on-dark); }
.pbtn:hover:not(:disabled) { background: var(--ink-3); border-color: var(--line-on-dark-strong); }
.pbtn:disabled { opacity: .4; }
.pbtn.main { background: var(--accent); border-color: var(--accent); color: #fff; }
.pbtn.main:hover { background: var(--accent-hover); border-color: var(--accent-hover); }
.clock { margin-left: 8px; font-size: 13px; min-width: 64px; color: #fff; font-variant-numeric: tabular-nums; white-space: nowrap; }
.speeds { display: inline-flex; border: 1px solid var(--line-on-dark-faint); border-radius: 7px; overflow: hidden; }
.sp { border: 0; background: transparent; color: var(--on-dark-muted); font-size: 11.5px; font-weight: 600; height: 26px; padding: 0 8px; }
.sp:hover { color: #fff; }
.sp.on { background: var(--ink-3); color: #fff; }
.track { position: relative; height: 22px; margin: 10px 8px 0; cursor: pointer; border-radius: 4px; }
.track:focus-visible { outline: 2px solid var(--accent-line); outline-offset: 4px; }
.rail, .fill { position: absolute; left: 0; top: 10px; height: 3px; border-radius: 2px; }
.rail { right: 0; background: var(--line-on-dark-faint); }
.fill { background: var(--accent); transition: width var(--dur-slow) var(--ease-out); }
.tick { position: absolute; top: 5px; width: 13px; height: 13px; margin-left: -6.5px; border-radius: 50%; border: 2px solid var(--line-on-dark-faint); background: var(--ink); padding: 0; transition: background-color var(--dur-base), border-color var(--dur-base), transform var(--dur-fast) var(--ease-spring); }
.tick.on { border-color: var(--accent); background: var(--accent); }
.tick.on.down { border-color: var(--crit); background: var(--crit); }
.tick.on.degraded { border-color: var(--risk-dot); background: var(--risk-dot); }
.tick.on.up { border-color: var(--ok-dot); background: var(--ok-dot); }
.tick.cur { transform: scale(1.35); box-shadow: 0 0 0 4px rgba(255, 255, 255, .12); }

/* the traffic: how much of the load is served in the picture on screen, and where the rest goes */
.traffic { margin: 4px 0 2px; }
.traffic h3 { margin-top: 12px; }
.big { display: flex; align-items: baseline; gap: 8px; flex-wrap: wrap; }
.num { font-size: 28px; line-height: 1; font-weight: 700; letter-spacing: -.02em; font-variant-numeric: tabular-nums; transition: color var(--dur-base); }
.num.ok { color: var(--ok); } .num.risk { color: var(--risk); } .num.crit { color: var(--crit); }
.what { font-size: 12.5px; color: var(--text-2); }
.was { margin-left: auto; font-size: 11.5px; color: var(--muted); }
.rps { margin: 4px 0 0; font-size: 12px; color: var(--muted); font-variant-numeric: tabular-nums; }
.bar { display: flex; height: 8px; margin: 9px 0 7px; border-radius: 4px; overflow: hidden; background: var(--surface-sunken); }
.bar i { display: block; height: 100%; transition: width var(--dur-slower) var(--ease-out), background-color var(--dur-base); }
.bar .ok { background: var(--flow); }
.bar .gone { background: var(--flow-lost); }
.bar.ok .ok { background: var(--ok-dot); }
.legend { display: flex; gap: 12px; font-size: 11.5px; color: var(--muted); }
.lg { display: inline-flex; align-items: center; gap: 5px; }
.lg i { width: 14px; height: 0; border-top: 3px solid var(--flow); border-radius: 2px; }
.lg.lost i { border-color: var(--flow-lost); }
.lg.missed i { border-color: var(--flow-missed); }
.lg.detour i { border-top-style: dashed; border-color: var(--flow-detour); }
.notes { list-style: none; padding: 0; margin: 8px 0 0; gap: 4px; }
.notes li { padding-left: 14px; position: relative; font-size: 12px; line-height: 1.4; }
.notes li::before { content: ''; position: absolute; left: 0; top: .5em; width: 7px; height: 7px; border-radius: 50%; background: var(--flow-lost); }
.notes li.missed::before { background: var(--flow-missed); }
.notes li.detour::before { background: var(--flow-detour); }
.notes li.more::before { display: none; }
.notes li.more { color: var(--muted); }

.steps { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 4px; }
.steps > li { display: grid; grid-template-columns: 62px 1fr auto; gap: 8px; padding: 6px 8px; margin: 0 -8px; border-radius: 8px; cursor: pointer; border-left: 3px solid transparent; transition: background-color var(--dur-fast), opacity var(--dur-base), border-color var(--dur-base); }
.steps > li:hover { background: var(--surface-sunken); }
.steps > li.cur { background: var(--accent-soft); border-left-color: var(--accent); }
.steps > li.future { opacity: .5; }
.at { font-size: 11.5px; color: var(--muted); padding-top: 1px; white-space: nowrap; }
.steps ul { padding-left: 14px; gap: 4px; }
.steps > li.before ul { list-style: none; padding-left: 0; }
.served { align-self: start; font-size: 11px; font-weight: 700; font-variant-numeric: tabular-nums; padding: 1px 7px; border-radius: 9px; background: var(--surface-sunken); color: var(--text-2); }
.served.ok { background: var(--ok-bg); color: var(--ok); }
.served.risk { background: var(--risk-bg); color: var(--risk); }
.served.crit { background: var(--crit-bg); color: var(--crit); }
</style>
