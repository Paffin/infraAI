import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { seenKey, type TourDef, type TourStep } from '@/lib/tour'
import { useApp } from './app'

const read = (k: string): string | null => { try { return localStorage.getItem(k) } catch { return null } }
const write = (k: string, v: string | null) => { try { if (v === null) localStorage.removeItem(k); else localStorage.setItem(k, v) } catch { /* private mode: the tour just comes back next time */ } }

/** "Do not show the tours again", for this browser. */
export const TOUR_OFF_KEY = 'ia_tour_off'

/**
 * The guided tour: which tour is running and where. Whether to start one by itself (a first visit, not switched off by the
 * administrator or by the user) is decided by `canAutostart`; the host component finds the elements and draws.
 */
export const useTour = defineStore('tour', () => {
  const app = useApp()
  const active = ref<{ def: TourDef; steps: TourStep[]; index: number } | null>(null)
  const off = ref(read(TOUR_OFF_KEY) === '1')
  /** The Help menu asks for the tour of the screen it is on; the host (which knows the DOM) answers and resets it. */
  const requested = ref(0)

  const step = computed(() => (active.value ? active.value.steps[active.value.index] ?? null : null))
  const count = computed(() => active.value?.steps.length ?? 0)
  const isLast = computed(() => !!active.value && active.value.index >= active.value.steps.length - 1)

  const seen = (def: TourDef) => read(seenKey(app.user?.id, def.id)) === '1'
  const markSeen = (def: TourDef) => write(seenKey(app.user?.id, def.id), '1')
  /** A tour starts by itself on the first visit of a screen, unless the administrator or the user turned them off. */
  const canAutostart = (def: TourDef) => app.onboarding && !off.value && !seen(def)

  function begin(def: TourDef, steps: TourStep[]) {
    if (steps.length === 0) return
    active.value = { def, steps, index: 0 }
  }
  /** The steps that can be shown changed (a panel appeared, a button went): keep the step on screen, count the rest again. */
  function setSteps(steps: TourStep[]) {
    const cur = active.value
    if (!cur || steps.length === 0) return
    if (steps.length === cur.steps.length && steps.every((s, i) => s.id === cur.steps[i].id)) return
    const id = cur.steps[cur.index]?.id
    const at = steps.findIndex((s) => s.id === id)
    active.value = { ...cur, steps, index: at >= 0 ? at : Math.min(cur.index, steps.length - 1) }
  }
  function go(i: number) {
    if (!active.value) return
    active.value = { ...active.value, index: Math.min(Math.max(0, i), active.value.steps.length - 1) }
  }
  function next() {
    if (!active.value) return
    if (isLast.value) finish()
    else go(active.value.index + 1)
  }
  const prev = () => active.value && go(active.value.index - 1)
  /** Closing (finished or skipped) counts as seen: a tour is a welcome, not a toll. */
  function finish() {
    if (active.value) markSeen(active.value.def)
    active.value = null
  }
  /** "Do not show again": no tour starts by itself in this browser any more (the Help menu still starts one). */
  function neverAgain() {
    off.value = true
    write(TOUR_OFF_KEY, '1')
    finish()
  }
  function forget() {
    off.value = false
    write(TOUR_OFF_KEY, null)
  }

  const request = () => { requested.value++ }

  return { active, step, count, isLast, off, requested, request, seen, canAutostart, begin, setSteps, go, next, prev, finish, neverAgain, forget }
})
