// Motion: which animations the interface plays, and the little maths JavaScript-driven ones need.
//
// `ui.motion` (setting) is the default for everybody, a user can choose their own in the View menu (kept in the browser).
// `system` means: follow the operating system's "reduce motion". The result is one of three modes, written to
// `<html data-motion>`, where the stylesheets pick it up (tokens.css, motion.css):
//   full     everything moves
//   reduced  no travel, no stagger, no loops: fades and colour changes only (what `prefers-reduced-motion` asks for)
//   off      nothing is animated at all (loading spinners excepted: they are feedback, not decoration)

export type MotionPref = 'system' | 'full' | 'reduced' | 'off'
export type MotionMode = 'full' | 'reduced' | 'off'

export const MOTION_PREFS: readonly MotionPref[] = ['system', 'full', 'reduced', 'off']

/** What the registry sends when it sends nothing (a backend that does not know the settings yet). */
export const FALLBACK_MOTION = { pref: 'system' as MotionPref, scale: 100 }

/** Limits of `ui.motionScale` (percent of the normal duration): the same as the registry's. */
export const MOTION_SCALE_MIN = 25
export const MOTION_SCALE_MAX = 300

export function parsePref(v: unknown): MotionPref | null {
  return typeof v === 'string' && (MOTION_PREFS as readonly string[]).includes(v) ? (v as MotionPref) : null
}

/** The mode in effect: the user's own choice, else the administrator's default; `system` asks the OS. */
export function resolveMode(user: MotionPref | null, adminDefault: MotionPref, systemReduces: boolean): MotionMode {
  const pref = user ?? adminDefault
  if (pref === 'system') return systemReduces ? 'reduced' : 'full'
  return pref
}

/** Percent of the normal duration → the multiplier of the stylesheets; anything unusable is the normal speed. */
export function scaleFactor(percent: unknown): number {
  const n = typeof percent === 'number' ? percent : Number(percent)
  if (!Number.isFinite(n)) return 1
  return Math.min(MOTION_SCALE_MAX, Math.max(MOTION_SCALE_MIN, n)) / 100
}

/** The duration tokens of tokens.css in milliseconds, for animations JavaScript runs itself (`tests/motion.test.ts` keeps them equal). */
export const DURATIONS = { instant: 80, fast: 140, base: 240, slow: 380, slower: 640 } as const

// ------------------------------------------------------------------------------------------ easing

export type Ease = (t: number) => number

export const ease = {
  linear: (t) => t,
  /** Fast start, soft landing: the default for things that arrive. */
  out: (t) => 1 - Math.pow(1 - t, 3),
  inOut: (t) => (t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2),
} satisfies Record<string, Ease>

/** One number moving towards another (`from + (to − from)·e(t)`). */
export function lerp(from: number, to: number, t: number): number {
  return from + (to - from) * t
}

/** Delay of the i-th item of a list that appears one after another, capped so a long list does not crawl. */
export function stagger(index: number, stepMs: number, maxMs: number): number {
  return Math.min(Math.max(0, index) * stepMs, maxMs)
}

/** `0.14s`, `140ms` or a list of them (`0.14s, 0.2s`, as `getComputedStyle` reports) → the longest, in milliseconds. */
export function parseCssTime(value: string | null | undefined): number {
  if (!value) return 0
  let longest = 0
  for (const part of value.split(',')) {
    const m = /^\s*(-?\d*\.?\d+)\s*(ms|s)\s*$/.exec(part)
    if (!m) continue
    const ms = parseFloat(m[1]) * (m[2] === 's' ? 1000 : 1)
    if (ms > longest) longest = ms
  }
  return longest
}

export interface Tween { cancel(): void }

/**
 * Calls `onUpdate` with values from `from` to `to` over `ms` (frame by frame) and returns a handle to stop it.
 * With no time (`ms <= 0`) the end value is delivered at once, synchronously: that is what the `off` mode relies on.
 */
export function tween(from: number, to: number, ms: number, onUpdate: (v: number) => void, easing: Ease = ease.out, raf: (cb: FrameRequestCallback) => number = requestAnimationFrame, cancelRaf: (h: number) => void = cancelAnimationFrame): Tween {
  if (!(ms > 0) || from === to) {
    onUpdate(to)
    return { cancel() { /* nothing is running */ } }
  }
  let handle = 0
  let start = 0
  let stopped = false
  const frame = (now: number) => {
    if (stopped) return
    if (!start) start = now
    const t = Math.min(1, (now - start) / ms)
    onUpdate(lerp(from, to, easing(t)))
    if (t < 1) handle = raf(frame)
  }
  handle = raf(frame)
  return { cancel() { stopped = true; cancelRaf(handle) } }
}
