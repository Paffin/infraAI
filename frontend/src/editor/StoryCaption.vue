<script setup lang="ts">
// The caption of the picture on screen while a failure is played back: over the canvas, where the eye is, what this moment is
// ("this is how the traffic goes without the failure" → "failure: DC-1" → what has happened since), with the controls to play, pause and step.
// The failure tab of the right panel has the same story in full.
import { computed } from 'vue'
import { ChevronLeft, ChevronRight, Pause, Play } from 'lucide-vue-next'
import { t } from '@/i18n'
import { eventText } from '@/lib/sim'
import { useEditor } from '@/stores/editor'
import { usePlayback } from '@/stores/playback'

const editor = useEditor()
const playback = usePlayback()

const kind = computed(() => playback.current?.kind ?? null)
const step = computed(() => playback.steps[playback.index])
const text = computed(() => {
  const res = editor.sim?.result
  if (!res || !kind.value) return ''
  if (kind.value === 'before') return t('caption.before')
  if (kind.value === 'failure') return t('caption.failure', { failed: res.failed.join(', ') }).replace(/:\s*$/, '')
  return step.value?.events[0] ? eventText(step.value.events[0]) : t('caption.settled')
})
const served = computed(() => (playback.served === null ? null : Math.round(playback.served * 100)))
const tone = computed(() => (kind.value === 'before' ? 'ok' : kind.value === 'failure' ? 'crit' : (playback.current?.state ?? 'up') === 'up' ? 'ok' : 'risk'))
const shown = computed(() => !!editor.sim?.result && playback.available && !editor.preview && !editor.pickingNode && !!text.value)
</script>

<template>
  <div v-if="shown" class="caption-wrap" data-overlay="caption">
    <div class="caption" :class="tone" data-testid="story-caption">
      <span class="ctl" role="group" :aria-label="t('play.title')">
        <button class="cb" :aria-label="t('play.prev')" :title="t('play.prev')" :disabled="playback.index === 0" @click="playback.step(-1)"><ChevronLeft :size="14" /></button>
        <button class="cb main" :aria-label="playback.playing ? t('play.pause') : t('play.play')" :title="playback.playing ? t('play.pause') : t('play.play')" data-testid="caption-play" @click="playback.toggle()"><component :is="playback.playing ? Pause : Play" :size="13" /></button>
        <button class="cb" :aria-label="t('play.next')" :title="t('play.next')" :disabled="playback.index >= playback.last" @click="playback.step(1)"><ChevronRight :size="14" /></button>
      </span>
      <Transition name="swap" mode="out-in">
        <span :key="playback.index" class="line" role="status" aria-live="polite">
          <i class="d" aria-hidden="true" />
          <span class="txt">{{ text }}</span>
          <b v-if="served !== null" class="pct">{{ served }}%</b>
        </span>
      </Transition>
    </div>
  </div>
</template>

<style scoped>
.caption-wrap { position: absolute; top: var(--bars-top, 56px); left: 0; right: 0; z-index: 57; display: flex; justify-content: center; pointer-events: none; padding: 0 16px; }
.caption { max-width: min(680px, 100%); display: flex; align-items: center; gap: 8px; padding: 4px 14px 4px 5px; border-radius: 999px; background: var(--surface); border: 1px solid var(--line-strong); box-shadow: var(--shadow-2); font-size: 13px; color: var(--text); pointer-events: auto; }
.ctl { display: inline-flex; align-items: center; gap: 2px; flex: none; }
.cb { width: 26px; height: 26px; border: 0; border-radius: 50%; background: transparent; color: var(--text-2); display: inline-flex; align-items: center; justify-content: center; transition: background-color var(--dur-fast), color var(--dur-fast); }
.cb:hover:not(:disabled) { background: var(--surface-sunken); color: var(--text); }
.cb:disabled { opacity: .35; }
.cb.main { background: var(--accent); color: #fff; }
.cb.main:hover { background: var(--accent-hover); color: #fff; }
.line { display: flex; align-items: center; gap: 9px; min-width: 0; }
.d { width: 9px; height: 9px; border-radius: 50%; background: var(--ok-dot); flex: none; box-shadow: 0 0 0 3px color-mix(in srgb, var(--ok-dot) 22%, transparent); }
.caption.crit .d { background: var(--crit); box-shadow: 0 0 0 3px var(--crit-bg); }
.caption.risk .d { background: var(--risk-dot); box-shadow: 0 0 0 3px var(--risk-bg); }
.txt { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 600; }
.pct { font-size: 12px; font-variant-numeric: tabular-nums; padding: 1px 8px; border-radius: 9px; background: var(--ok-bg); color: var(--ok); flex: none; }
.caption.crit .pct { background: var(--crit-bg); color: var(--crit); }
.caption.risk .pct { background: var(--risk-bg); color: var(--risk); }
</style>
