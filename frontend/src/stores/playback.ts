import { defineStore } from 'pinia'
import { computed, ref, watch } from 'vue'
import type { EdgeState, NodeState } from '@/api/types'
import { ease, tween, type Tween } from '@/lib/motion'
import { pictures, stepsOf } from '@/lib/playback'
import { useApp } from './app'
import { useEditor } from './editor'
import { useMotion } from './motion'

/** Speeds the failure can be played at: a multiple of the settled step length (`editor.playbackStepMs`). */
export const PLAYBACK_SPEEDS = [1, 2, 4] as const

/**
 * Playback of the failure the editor is showing: the frames of `SimResult.timeline` one after another. A result is shown
 * *settled* (its last frame: what the diagram showed before playback existed) until the user plays or scrubs it.
 * The diagram (nodes, edges) reads the states to draw from here, so a step is simply a different picture.
 */
export const usePlayback = defineStore('playback', () => {
  const editor = useEditor()
  const app = useApp()
  const motion = useMotion()

  const result = computed(() => editor.sim?.result ?? null)
  const all = computed(() => pictures(result.value?.timeline ?? []))
  const steps = computed(() => (result.value ? stepsOf(result.value) : []))
  /** Is there a story to tell? (a failure that is over at once has none) */
  const available = computed(() => all.value.length > 1)
  const last = computed(() => Math.max(0, all.value.length - 1))

  const index = ref(0)
  const playing = ref(false)
  const speed = ref<number>(PLAYBACK_SPEEDS[0])
  /** The seconds the clock shows: it runs from one step's moment to the next while the step is on screen. */
  const clock = ref(0)

  let timer: ReturnType<typeof setTimeout> | undefined
  let clockTween: Tween | null = null

  const stepMs = computed(() => app.editorCfg.playbackStepMs / speed.value)
  const current = computed(() => (available.value ? all.value[Math.min(index.value, last.value)] : null))

  /** What the diagram draws: the picture of the current step, or the settled result of a failure without a story. */
  const nodeStates = computed<Record<string, NodeState>>(() => current.value?.nodes ?? result.value?.nodeStates ?? {})
  const edgeStates = computed<Record<string, EdgeState>>(() => current.value?.edges ?? result.value?.edgeStates ?? {})
  /** Nodes that changed in the step on screen (they pulse), and the step's number (the pulse restarts on every step). */
  const changed = computed(() => new Set(index.value > 0 && current.value ? current.value.changed : []))

  function stopTimers() {
    clearTimeout(timer)
    timer = undefined
    clockTween?.cancel()
    clockTween = null
  }

  /** Show step `i`; the clock runs there over most of the step (in `full` motion), else it is simply there. */
  function show(i: number, animate: boolean) {
    const to = Math.min(Math.max(0, i), last.value)
    const from = clock.value
    index.value = to
    const at = all.value[to]?.at ?? 0
    clockTween?.cancel()
    if (animate && motion.full && from !== at) clockTween = tween(from, at, stepMs.value * 0.7 * motion.factor, (v) => { clock.value = v }, ease.inOut)
    else clock.value = at
  }

  function scheduleNext() {
    clearTimeout(timer)
    timer = setTimeout(() => {
      if (!playing.value) return
      if (index.value >= last.value) { playing.value = false; return }
      show(index.value + 1, true)
      if (index.value >= last.value) { playing.value = false; return }
      scheduleNext()
    }, stepMs.value * motion.factor)
  }

  function play() {
    if (!available.value) return
    if (index.value >= last.value) show(0, false)
    playing.value = true
    scheduleNext()
  }
  function pause() {
    playing.value = false
    clearTimeout(timer)
  }
  function toggle() {
    if (playing.value) pause()
    else play()
  }
  /** The user takes the time into their own hands. */
  function seek(i: number) {
    pause()
    show(i, false)
  }
  function step(delta: number) {
    seek(index.value + delta)
  }
  function setSpeed(s: number) {
    speed.value = s
    if (playing.value) scheduleNext()
  }
  function reset() {
    pause()
    stopTimers()
    index.value = last.value
    clock.value = all.value[last.value]?.at ?? 0
  }

  // a new failure is shown settled; the same failure re-run after an edit keeps the moment the user was looking at
  watch(() => (result.value ? JSON.stringify(result.value.scenario) : null), () => reset(), { immediate: true })
  watch(all, (frames, before) => {
    if (playing.value) return
    const wasSettled = index.value >= before.length - 1
    show(wasSettled ? frames.length - 1 : Math.min(index.value, frames.length - 1), false)
  })

  return { result, available, steps, index, last, playing, speed, clock, current, nodeStates, edgeStates, changed, stepMs, play, pause, toggle, seek, step, setSpeed, reset }
})
