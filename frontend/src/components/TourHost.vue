<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import { t } from '@/i18n'
import { holeOf, placeCard, stepKeys, tourFor, visibleSteps, type Rect, type TourDef } from '@/lib/tour'
import { useApp } from '@/stores/app'
import { useTour } from '@/stores/tour'
import { useUi } from '@/stores/ui'

const app = useApp()
const tour = useTour()
const route = useRoute()
const ui = useUi()

const PAD = 8
const target = ref<Rect | null>(null)
const view = ref({ w: window.innerWidth, h: window.innerHeight })
const cardEl = ref<HTMLElement | null>(null)
const cardSize = ref({ w: 360, h: 190 })

const step = computed(() => tour.step)
const keys = computed(() => (tour.active && step.value ? stepKeys(tour.active.def.id, step.value.id) : null))
const hole = computed(() => (target.value ? holeOf(target.value, PAD, view.value) : null))
const place = computed(() => placeCard(hole.value, cardSize.value, view.value, step.value?.placement ?? 'bottom'))

// ---- finding the element and keeping the picture in step with the layout ----------------------------------------------
function el(selector: string | undefined): HTMLElement | null {
  if (!selector) return null
  const found = document.querySelector<HTMLElement>(selector)
  return found && found.offsetParent !== null ? found : found && getComputedStyle(found).position === 'fixed' ? found : null
}
function measure() {
  view.value = { w: window.innerWidth, h: window.innerHeight }
  // the screen is still filling in (the findings, the what-if bar): steps that can be shown now may differ from those at the start
  if (tour.active) tour.setSteps(visibleSteps(tour.active.def, present, app.isAdmin))
  const e = el(step.value?.target)
  if (e) {
    const r = e.getBoundingClientRect()
    target.value = { x: r.left, y: r.top, w: r.width, h: r.height }
  } else {
    target.value = null
  }
  if (cardEl.value) cardSize.value = { w: cardEl.value.offsetWidth, h: cardEl.value.offsetHeight }
}

/** A step may need the screen arranged first: the library open, the tab with the findings shown. */
function prepare(id: string) {
  if (id === 'library') ui.libraryOpen = true
  if (id === 'checks') ui.tab = 'checks'
}

let ro: ResizeObserver | undefined
let tick: ReturnType<typeof setInterval> | undefined
async function settle() {
  const s = step.value
  if (!s) return
  prepare(s.id)
  await nextTick()
  el(s.target)?.scrollIntoView?.({ block: 'nearest', inline: 'nearest' })
  measure()
  ro?.disconnect()
  const e = el(s.target)
  if (e && typeof ResizeObserver !== 'undefined') { ro = new ResizeObserver(measure); ro.observe(e) }
  await nextTick()
  cardEl.value?.focus?.({ preventScroll: true })
}
watch(step, () => void settle())
/** What is behind the tour cannot be reached while it is open: not by the mouse, not by Tab, not by a screen reader. */
function lock(on: boolean) {
  const root = document.getElementById('app')
  if (!root) return
  if (on) root.setAttribute('inert', '')
  else root.removeAttribute('inert')
}
watch(() => tour.active !== null, (on) => {
  lock(on)
  clearInterval(tick)
  if (on) { tick = setInterval(measure, 300); void settle() } else { ro?.disconnect(); target.value = null }
})

// ---- starting: by itself on a first visit, or on request (the Help menu) ---------------------------------------------
/** Panels the tour opens itself (the collapsed library) count as there while they only exist in the page. */
const OPENABLE = ['[data-tour="library"]']
function present(selector: string) { return OPENABLE.includes(selector) ? !!document.querySelector(selector) : !!el(selector) }

/** Waits for the screen to be there (the editor loads its project first), then starts the tour with the steps that can be shown. */
async function launch(def: TourDef, force: boolean) {
  const first = def.steps.find((s) => s.target)
  const deadline = Date.now() + 6000
  while (first?.target && !present(first.target) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 150))
  if (route.name !== def.route || tour.active) return
  if (!force && !tour.canAutostart(def)) return
  tour.begin(def, visibleSteps(def, present, app.isAdmin))
}
defineExpose({ launch })

watch(
  () => [route.name, app.user?.id, !!app.boot] as const,
  ([name, uid, booted]) => {
    const def = tourFor(name)
    if (def && uid && booted && !tour.active && tour.canAutostart(def)) void launch(def, false)
  },
  { immediate: true },
)
// the Help menu asks for a tour of the screen it is on
watch(() => tour.requested, (req) => {
  const def = req ? tourFor(route.name) : undefined
  tour.requested = 0
  if (def) void launch(def, true)
})

function onKey(e: KeyboardEvent) {
  if (!tour.active) return
  // the tour owns the keyboard while it is open: nothing underneath (a dialog's Esc, a shortcut) hears these keys
  const own = (fn: () => void) => { e.preventDefault(); e.stopImmediatePropagation(); fn() }
  if (e.key === 'Escape') own(() => tour.finish())
  else if (e.key === 'ArrowRight') own(() => tour.next())
  else if (e.key === 'ArrowLeft') own(() => tour.prev())
  else if (e.key === 'Enter' && !(e.target instanceof HTMLButtonElement)) own(() => tour.next())
  else if (e.key === 'Tab') {
    // focus stays on the card: Tab goes round its buttons
    const items = Array.from(cardEl.value?.querySelectorAll<HTMLElement>('button:not([disabled])') ?? [])
    if (!items.length) return
    const at = items.indexOf(document.activeElement as HTMLElement)
    const to = e.shiftKey ? (at <= 0 ? items.length - 1 : at - 1) : (at < 0 || at === items.length - 1 ? 0 : at + 1)
    own(() => items[to].focus())
  }
}
onMounted(() => { window.addEventListener('keydown', onKey, true); window.addEventListener('resize', measure); window.addEventListener('scroll', measure, true) })
onBeforeUnmount(() => {
  lock(false)
  window.removeEventListener('keydown', onKey, true)
  window.removeEventListener('resize', measure)
  window.removeEventListener('scroll', measure, true)
  clearInterval(tick)
  ro?.disconnect()
})
// leaving the screen ends its tour
watch(() => route.name, () => { if (tour.active && tour.active.def.route !== route.name) tour.active = null })
</script>

<template>
  <Teleport to="body">
    <Transition name="fade">
      <div v-if="tour.active && step && keys" class="tour" role="dialog" aria-modal="true" :aria-labelledby="'tour-title'" data-testid="tour">
        <div class="veil" @mousedown.stop.prevent />
        <div v-if="hole" class="hole" :style="{ left: hole.x + 'px', top: hole.y + 'px', width: hole.w + 'px', height: hole.h + 'px' }" />
        <div v-else class="dim" />
        <div ref="cardEl" class="card" tabindex="-1" :class="{ centered: place.placement === 'center' }" :style="{ left: place.x + 'px', top: place.y + 'px' }">
          <Transition name="swap" mode="out-in">
            <div :key="step.id" class="inner">
              <div class="count mono">{{ t('tour.step', { i: tour.active.index + 1, n: tour.count }) }}</div>
              <h3 id="tour-title">{{ t(keys.title) }}</h3>
              <p>{{ t(keys.body) }}</p>
            </div>
          </Transition>
          <div class="foot">
            <button v-if="!tour.isLast" class="btn btn-ghost btn-sm" data-testid="tour-skip" @click="tour.finish()">{{ t('tour.skip') }}</button>
            <span class="grow" />
            <button v-if="tour.active.index > 0" class="btn btn-sm" data-testid="tour-prev" @click="tour.prev()">{{ t('tour.prev') }}</button>
            <button class="btn btn-primary btn-sm" data-testid="tour-next" @click="tour.next()">{{ tour.isLast ? t('tour.done') : t('tour.next') }}</button>
          </div>
          <div class="meta">
            <div class="dots" aria-hidden="true"><i v-for="n in tour.count" :key="n" :class="{ on: n - 1 <= tour.active.index, cur: n - 1 === tour.active.index }" /></div>
            <button class="link" data-testid="tour-never" @click="tour.neverAgain()">{{ t('tour.never') }}</button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.tour { position: fixed; inset: 0; z-index: 300; }
.veil { position: absolute; inset: 0; }
.dim { position: absolute; inset: 0; background: var(--veil); }
.hole { position: absolute; border-radius: 10px; box-shadow: 0 0 0 100vmax var(--veil), 0 0 0 2px var(--accent-line) inset; pointer-events: none; transition: left var(--dur-slow) var(--ease-out), top var(--dur-slow) var(--ease-out), width var(--dur-slow) var(--ease-out), height var(--dur-slow) var(--ease-out); }
.card { position: absolute; width: 360px; max-width: calc(100vw - 24px); background: var(--surface); color: var(--text); border-radius: var(--radius-lg); box-shadow: var(--shadow-3); padding: 16px 18px 14px; outline: none; transition: left var(--dur-slow) var(--ease-out), top var(--dur-slow) var(--ease-out); }
.count { font-size: 11px; letter-spacing: .06em; text-transform: uppercase; color: var(--muted); }
h3 { margin: 4px 0 6px; font-size: 16px; letter-spacing: -.01em; }
p { margin: 0 0 14px; font-size: 13.5px; line-height: 1.55; color: var(--text-2); min-height: 62px; }
.foot { display: flex; align-items: center; gap: 6px; }
.link { border: 0; background: transparent; padding: 0; color: var(--muted); font-size: 11.5px; text-decoration: underline; text-underline-offset: 2px; white-space: nowrap; }
.link:hover { color: var(--text); }
.meta { display: flex; align-items: center; justify-content: space-between; gap: 10px; margin-top: 12px; }
.dots { display: flex; gap: 5px; }
.dots i { width: 6px; height: 6px; border-radius: 50%; background: var(--line-strong); transition: background-color var(--dur-base), transform var(--dur-fast) var(--ease-spring); }
.dots i.on { background: var(--accent-line); }
.dots i.cur { background: var(--accent); transform: scale(1.35); }
</style>
