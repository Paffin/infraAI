import type { InjectionKey, Ref } from 'vue'

/**
 * Level of detail of the canvas by zoom ("semantic zoom"). Zoomed out, a card drops its subtitle and keeps its
 * name at a legible screen size (`compact`); further out it becomes a coloured tile (`mini`) – the overview of a
 * big architecture stays readable instead of turning into grey specks. The thresholds are settings
 * (`editor.lodCompactBelow`, `editor.lodMiniBelow`).
 */
export type Lod = 'full' | 'compact' | 'mini'

/**
 * The canvas hands its level of detail to the cards and the heads of zones (provide / inject): what a card writes on itself depends on it (the sub-line goes at
 * `compact`, the name too at `mini`, where the abbreviation of the kind is all that is left), and so must its accessible name, which has to contain the words that are
 * written (WCAG 2.5.3). It changes only when the zoom crosses a threshold, so a card that reads it is not redrawn by every step of a zoom.
 */
export const LOD_KEY: InjectionKey<Ref<Lod>> = Symbol('lod')

export function lodOf(zoom: number, limits: { lodCompactBelow: number; lodMiniBelow: number }): Lod {
  return zoom < limits.lodMiniBelow ? 'mini' : zoom < limits.lodCompactBelow ? 'compact' : 'full'
}
