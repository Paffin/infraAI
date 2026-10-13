<script setup lang="ts">
import { computed } from 'vue'
import { ChevronLeft, ChevronRight, Pause, Play, RotateCcw, X } from 'lucide-vue-next'
import { t } from '@/i18n'
import { fmtDuration } from '@/lib/format'
import { stepAtPercent, trackPosition } from '@/lib/playback'
import { eventText, scenarioLabel } from '@/lib/sim'
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
const clockText = computed(() => (playback.clock < 0.5 ? t('play.now') : '+' + fmtDuration(Math.round(playback.clock))))
const atText = (at: number) => (at <= 0 ? t('play.now') : '+' + fmtDuration(at))

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
  <div v-if="sim && res" class="simcard card" role="status" data-testid="simcard">
    <header>
      <span class="dot" /><b class="grow truncate">{{ t('sim.banner', { failed: res.failed.length ? res.failed.join(', ') : title }) }}</b>
      <button class="btn btn-ghost btn-icon" :aria-label="t('sim.exit')" :title="t('sim.exit')" @click="editor.exitSim()"><X :size="16" /></button>
    </header>
    <p class="state" :class="shownState" data-testid="sim-state">{{ t('sim.result.' + shownState) }}<span v-if="!playback.available && res.downUnits.length"> · {{ res.downUnits.join(', ') }}</span></p>

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
        <button v-for="s in steps" :key="s.index" class="tick" :class="[s.state, { on: s.index <= playback.index, cur: s.index === playback.index }]" :style="{ left: trackPosition(s.index, count) + '%' }" :aria-label="atText(s.at)" :title="atText(s.at)" tabindex="-1" @click.stop="playback.seek(s.index)" />
      </div>
    </section>

    <h5>{{ t('sim.details') }}</h5>
    <ol v-if="playback.available" class="steps">
      <li v-for="s in steps" :key="s.index" :class="{ cur: s.index === playback.index, past: s.index < playback.index, future: s.index > playback.index }" :data-step="s.index" @click="playback.seek(s.index)">
        <span class="at mono">{{ atText(s.at) }}</span>
        <ul>
          <li v-for="(e, i) in s.events" :key="i">{{ eventText(e) }}</li>
          <li v-if="!s.events.length" class="muted">{{ t('play.settled') }}</li>
        </ul>
      </li>
    </ol>
    <ul v-else>
      <li v-for="(e, i) in res.events" :key="i">{{ eventText(e) }}</li>
      <li v-if="!res.events.length" class="muted">{{ t('sim.noevents') }}</li>
    </ul>
    <p v-if="res.rpoAssumed" class="muted small">{{ t('sim.assumed') }}</p>
  </div>
</template>

<style scoped>
.simcard { position: absolute; right: 18px; top: 62px; width: 346px; max-height: calc(100% - 270px); overflow: auto; z-index: 55; padding: 12px 16px 14px; box-shadow: var(--shadow-2); border-color: var(--crit-line); }
header { display: flex; align-items: center; gap: 8px; margin: -4px -8px 2px 0; font-size: 14px; }
.dot { width: 9px; height: 9px; border-radius: 50%; background: var(--crit); flex: none; }
.state { margin: 0 0 8px; font-weight: 600; font-size: 13px; transition: color var(--dur-base); }
.state.up { color: var(--ok); } .state.degraded { color: var(--risk); } .state.down { color: var(--crit); }
h5 { margin: 10px 0 6px; font-size: 10.5px; letter-spacing: .09em; text-transform: uppercase; color: #6e7fa8; font-weight: 600; }
ul { margin: 0; padding-left: 16px; display: flex; flex-direction: column; gap: 6px; font-size: 12.5px; line-height: 1.45; color: var(--text-2); }
.small { font-size: 12px; margin: 8px 0 0; }

.player { background: var(--ink); color: var(--on-dark); border-radius: 10px; padding: 8px 10px 12px; margin: 2px 0 4px; }
.ctl { display: flex; align-items: center; gap: 4px; }
.pbtn { width: 28px; height: 28px; display: inline-flex; align-items: center; justify-content: center; border: 1px solid var(--line-on-dark-faint); border-radius: 7px; background: transparent; color: var(--on-dark); }
.pbtn:hover:not(:disabled) { background: var(--ink-3); border-color: var(--line-on-dark-strong); }
.pbtn:disabled { opacity: .4; }
.pbtn.main { background: var(--accent); border-color: var(--accent); color: #fff; }
.pbtn.main:hover { background: var(--accent-hover); border-color: var(--accent-hover); }
.clock { margin-left: 8px; font-size: 13px; min-width: 64px; color: #fff; font-variant-numeric: tabular-nums; }
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

.steps { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 4px; }
.steps > li { display: grid; grid-template-columns: 54px 1fr; gap: 8px; padding: 6px 8px; margin: 0 -8px; border-radius: 8px; cursor: pointer; border-left: 3px solid transparent; transition: background-color var(--dur-fast), opacity var(--dur-base), border-color var(--dur-base); }
.steps > li:hover { background: var(--surface-sunken); }
.steps > li.cur { background: var(--accent-soft); border-left-color: var(--accent); }
.steps > li.future { opacity: .5; }
.at { font-size: 11.5px; color: var(--muted); padding-top: 1px; white-space: nowrap; }
.steps ul { padding-left: 14px; gap: 4px; }
@container (max-width: 1260px) { .simcard { top: 104px; max-height: calc(100% - 280px); } }
</style>
