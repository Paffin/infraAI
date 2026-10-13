import { defineStore } from 'pinia'
import { computed, ref, watch } from 'vue'
import type { EdgeState, NodeState, Traffic } from '@/api/types'
import { ease, tween, type Tween } from '@/lib/motion'
import { stepsOf, story } from '@/lib/playback'
import { detourPairs, loadChanges, particleCount, reference } from '@/lib/traffic'
import { useApp } from './app'
import { useEditor } from './editor'
import { useMotion } from './motion'

/** Speeds the failure can be played at: a multiple of the settled step length (`editor.playbackStepMs`). */
export const PLAYBACK_SPEEDS = [1, 2, 4] as const

/**
 * Playback of the failure the editor is showing: the pictures of its story one after another. The story starts with normal operation (the
 * traffic as it goes without the failure), then the moment of the failure and every step the recovery takes (`SimResult.timeline`).
 * A failure that has just been run plays itself from the normal picture to the outcome (`editor.failureLeadMs`; with full animation);
 * otherwise a result is shown *settled* (its last picture) until the user plays or scrubs it.
 * The diagram (nodes, edges, the traffic on them) reads what to draw from here, so a step is simply a different picture.
 */
export const usePlayback = defineStore('playback', () => {
  const editor = useEditor()
  const app = useApp()
  const motion = useMotion()

  const result = computed(() => editor.sim?.result ?? null)
  const all = computed(() => (result.value ? story(result.value) : []))
  const steps = computed(() => (result.value ? stepsOf(result.value) : []))
  /** Is there a story to tell? (a failure that is over at once, in a backend that does not tell where the traffic goes, has none) */
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
  /** The picture on screen is normal operation, before the failure. */
  const beforeFailure = computed(() => current.value?.kind === 'before')
  /** How long the picture at `i` stays: normal operation is held a little longer (`editor.failureLeadMs`), so that the traffic can be watched. */
  const holdMs = (i: number) => {
    const lead = app.editorCfg.failureLeadMs
    return (all.value[i]?.kind === 'before' && lead > 0 ? lead : app.editorCfg.playbackStepMs) / speed.value
  }

  /** What the diagram draws: the picture of the current step, or the settled result of a failure without a story. */
  const nodeStates = computed<Record<string, NodeState>>(() => current.value?.nodes ?? result.value?.nodeStates ?? {})
  const edgeStates = computed<Record<string, EdgeState>>(() => current.value?.edges ?? result.value?.edgeStates ?? {})
  /** Nodes that changed in the step on screen (they pulse), and the step's number (the pulse restarts on every step). */
  const changed = computed(() => new Set(index.value > 0 && current.value ? current.value.changed : []))

  // ---- the traffic
  /** Where the traffic goes in the picture on screen; `null` when the engine did not tell (or nothing is simulated). */
  const traffic = computed<Traffic | null>(() => current.value?.traffic ?? result.value?.traffic ?? null)
  /** Normal operation, to compare the picture with. */
  const before = computed<Traffic | null>(() => result.value?.trafficBefore ?? null)
  /** What widths and densities are measured against: the same in every picture of the story. */
  const flowRef = computed(() => reference(before.value, result.value?.traffic, ...all.value.map((p) => p.traffic)))
  /** The nodes whose load is not what it is in normal operation. */
  const loads = computed(() => (traffic.value && before.value ? loadChanges(before.value, traffic.value, editor.shown) : {}))
  /** Every detour of the story: the canvas has an edge for each, drawn only in the pictures that have it. */
  const detours = computed(() => detourPairs(result.value))
  /**
   * 1 when every particle fits `editor.flowParticlesMax`, less when the diagram is so big that the flow has to thin out; 0 when particles
   * are switched off. The lengths are the distances between the cards: close enough to budget by.
   */
  const particleScale = computed(() => {
    const max = app.editorCfg.flowParticlesMax
    const t = traffic.value
    if (!t || max <= 0) return 0
    const pos = new Map(editor.shown.nodes.map((n) => [n.id, { x: n.ui?.x ?? 0, y: n.ui?.y ?? 0 }]))
    const dist = (a: string, b: string) => {
      const p = pos.get(a)
      const q = pos.get(b)
      return p && q ? Math.hypot(p.x - q.x, p.y - q.y) : 0
    }
    let total = 0
    for (const e of editor.shown.edges) total += particleCount((t.flows[e.id] ?? 0) + (t.lost?.[e.id] ?? 0), flowRef.value, dist(e.source, e.target), app.editorCfg.flowSpacing)
    for (const d of t.detours ?? []) total += particleCount(d.flow + (d.lost ?? 0), flowRef.value, dist(d.from, d.to), app.editorCfg.flowSpacing)
    return total <= max ? 1 : max / total
  })
  const served = computed(() => traffic.value?.served ?? null)

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
    const at = all.value[to]?.kind === 'before' ? 0 : (all.value[to]?.at ?? 0)
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
    }, holdMs(index.value) * motion.factor)
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
  /** A failure that has just been run tells its story by itself (if the administrator and the animation mode allow it); else it is shown settled. */
  function reset() {
    pause()
    stopTimers()
    if (available.value && all.value[0]?.kind === 'before' && app.editorCfg.failureLeadMs > 0 && motion.full) {
      show(0, false)
      playing.value = true
      scheduleNext()
      return
    }
    index.value = last.value
    clock.value = all.value[last.value]?.at ?? 0
  }

  // a new failure is shown from the start; the same failure re-run after an edit keeps the moment the user was looking at
  watch(() => (result.value ? JSON.stringify(result.value.scenario) : null), () => reset(), { immediate: true })
  watch(all, (frames, before) => {
    if (playing.value) return
    const wasSettled = index.value >= before.length - 1
    show(wasSettled ? frames.length - 1 : Math.min(index.value, frames.length - 1), false)
  })

  return {
    result, available, steps, index, last, playing, speed, clock, current, beforeFailure, nodeStates, edgeStates, changed, stepMs, holdMs,
    traffic, before, flowRef, loads, detours, particleScale, served,
    play, pause, toggle, seek, step, setSpeed, reset,
  }
})
