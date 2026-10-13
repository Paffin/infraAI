// How much room the longest word of a name needs. A card's name wraps at spaces and nowhere else (a word cut in the middle reads as a typo),
// so when the diagram is zoomed out and the name is drawn larger than the card (the semantic zoom in Canvas.vue keeps it readable), the font
// size has to come down until the longest word fits. The CSS needs that word's width in ems for this; it is measured here with the font the
// card is set in, and again once the web fonts have loaded (a width measured with the fallback font is not the width of the real one).

import { ref } from 'vue'

/** Bumps when the fonts have loaded, so that what was measured before is measured again. */
const epoch = ref(0)
if (typeof document !== 'undefined' && document.fonts) {
  void document.fonts.ready.then(() => { epoch.value++ })
  document.fonts.addEventListener?.('loadingdone', () => { epoch.value++ })
}

/** The words of a name as it wraps: at spaces, and after a hyphen. */
export const wordsOf = (text: string) => text.replace(/-/g, '- ').split(/\s+/).filter(Boolean)

/** An average letter of the UI font in ems, for where there is no canvas to measure with (the unit tests run without a browser). */
const AVERAGE_EM = 0.62
let canvas: CanvasRenderingContext2D | null | undefined
function context(): CanvasRenderingContext2D | null {
  if (canvas === undefined) {
    try { canvas = typeof navigator !== 'undefined' && /jsdom|happy-dom/i.test(navigator.userAgent) ? null : document.createElement('canvas').getContext('2d') } catch { canvas = null }
  }
  return canvas
}

const cache = new Map<string, number>()

/** The width of the longest word of `text`, in ems, at `weight` (the weight of a card's name, ArchNode.vue). Never less than half an em. */
export function longestWordEm(text: string, weight = 620): number {
  const key = `${epoch.value}|${weight}|${text}`
  const hit = cache.get(key)
  if (hit !== undefined) return hit
  const words = wordsOf(text)
  const c = context()
  let em = 0
  if (c) {
    c.font = `${weight} 100px ${getComputedStyle(document.body).fontFamily}`
    for (const w of words) em = Math.max(em, c.measureText(w).width / 100)
  } else {
    for (const w of words) em = Math.max(em, w.length * AVERAGE_EM)
  }
  em = Math.max(0.5, Math.round(em * 100) / 100)
  if (cache.size > 2000) cache.clear()
  cache.set(key, em)
  return em
}
