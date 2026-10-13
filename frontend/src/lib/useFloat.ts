// A panel the user can drag by a grip anywhere over an area (the canvas), and put back where it came from. Where it was left is remembered
// per browser (`key`) as a fraction of the free room, so it is where it was and in view whatever the window does (lib/floating.ts).
// The panel keeps its top-left when its own size changes (it folds, it grows), and keeps its relative place when the area changes.
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { clampPos, fracOf, parseFrac, placeFrac, type Frac, type Pos } from './floating'

const read = (k: string): string | null => { try { return localStorage.getItem(k) } catch { return null } }
const write = (k: string, v: string | null) => { try { if (v === null) localStorage.removeItem(k); else localStorage.setItem(k, v) } catch { /* private mode */ } }

/** How far the arrow keys move a panel, and how far with Shift. */
const STEP = 16
const BIG_STEP = 64

export function useFloat(opts: { key: string; area: () => HTMLElement | null; el: () => HTMLElement | null; margin?: number }) {
  const margin = opts.margin ?? 10
  const pos = ref<Pos | null>(null)
  const dragging = ref(false)
  let frac: Frac | null = parseFrac(read(opts.key))
  let drag: { px: number; py: number; ox: number; oy: number } | null = null
  let areaWatch: ResizeObserver | undefined
  let elWatch: ResizeObserver | undefined

  function sizes() {
    const a = opts.area()
    const e = opts.el()
    if (!a || !e) return null
    const ar = a.getBoundingClientRect()
    const er = e.getBoundingClientRect()
    return { ar, er, area: { w: ar.width, h: ar.height }, panel: { w: er.width, h: er.height } }
  }
  /** Where the panel is now, relative to the area: where it was put, or where its own style has it. */
  function current(): Pos | null {
    if (pos.value) return pos.value
    const s = sizes()
    return s ? { x: s.er.left - s.ar.left, y: s.er.top - s.ar.top } : null
  }
  function set(p: Pos) {
    const s = sizes()
    if (!s) return
    const c = clampPos(p, s.area, s.panel, margin)
    pos.value = c
    frac = fracOf(c, s.area, s.panel, margin)
  }

  /** The area changed: the panel keeps its relative place. */
  function onArea() {
    if (!frac) return
    const s = sizes()
    if (s) pos.value = placeFrac(frac, s.area, s.panel, margin)
  }
  /** The panel changed: it keeps its top-left, and stays in view. */
  function onPanel() {
    if (pos.value && !drag) set(pos.value)
  }

  /**
   * Take hold of the panel. The move and the end are listened to on the window (in the capture phase, because the canvas stops the events of its
   * own area) rather than by capturing the pointer: a pointer capture would also steal the clicks, and the double click on the grip that puts the panel back.
   */
  function onPointerDown(e: PointerEvent) {
    if (e.button !== 0 || drag) return
    const c = current()
    if (!c) return
    e.preventDefault()
    drag = { px: e.clientX, py: e.clientY, ox: c.x, oy: c.y }
    dragging.value = true
    window.addEventListener('pointermove', onPointerMove, true)
    window.addEventListener('pointerup', onPointerUp, true)
    window.addEventListener('pointercancel', onPointerUp, true)
  }
  function onPointerMove(e: PointerEvent) {
    if (!drag) return
    set({ x: drag.ox + e.clientX - drag.px, y: drag.oy + e.clientY - drag.py })
  }
  function onPointerUp() {
    release()
    if (frac) write(opts.key, JSON.stringify(frac))
  }
  function release() {
    drag = null
    dragging.value = false
    window.removeEventListener('pointermove', onPointerMove, true)
    window.removeEventListener('pointerup', onPointerUp, true)
    window.removeEventListener('pointercancel', onPointerUp, true)
  }
  /** Arrow keys move the panel, Home puts it back. */
  function onKeyDown(e: KeyboardEvent) {
    const step = e.shiftKey ? BIG_STEP : STEP
    const d: Record<string, [number, number]> = { ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, -step], ArrowDown: [0, step] }
    if (e.key === 'Home') { e.preventDefault(); reset(); return }
    const m = d[e.key]
    const c = current()
    if (!m || !c) return
    e.preventDefault()
    set({ x: c.x + m[0], y: c.y + m[1] })
    if (frac) write(opts.key, JSON.stringify(frac))
  }
  /** Back to where the panel sits by default (its own style). */
  function reset() {
    pos.value = null
    frac = null
    write(opts.key, null)
  }

  onMounted(() => {
    onArea()
    const a = opts.area()
    const e = opts.el()
    if (typeof ResizeObserver !== 'undefined') {
      if (a) { areaWatch = new ResizeObserver(onArea); areaWatch.observe(a) }
      if (e) { elWatch = new ResizeObserver(onPanel); elWatch.observe(e) }
    }
  })
  onBeforeUnmount(() => { areaWatch?.disconnect(); elWatch?.disconnect(); release() })

  /** The style that puts the panel where it was left (nothing while it sits by default). */
  const style = computed(() => (pos.value ? { left: pos.value.x + 'px', top: pos.value.y + 'px', right: 'auto', bottom: 'auto', transform: 'none' } : {}))
  const floating = computed(() => pos.value !== null)
  /** Is the panel in the lower half of the area? (menus open upwards there, downwards otherwise) */
  const lowerHalf = computed(() => {
    const s = pos.value && sizes()
    return !s || !pos.value || pos.value.y + s.panel.h / 2 > s.area.h / 2
  })
  return { pos, dragging, floating, style, lowerHalf, onPointerDown, onKeyDown, reset, place: onArea }
}
