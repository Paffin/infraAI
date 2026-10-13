/**
 * Level of detail of the canvas by zoom ("semantic zoom"). Zoomed out, a card drops its subtitle and keeps its
 * name at a legible screen size (`compact`); further out it becomes a coloured tile (`mini`) – the overview of a
 * big architecture stays readable instead of turning into grey specks. The thresholds are settings
 * (`editor.lodCompactBelow`, `editor.lodMiniBelow`).
 */
export type Lod = 'full' | 'compact' | 'mini'

export function lodOf(zoom: number, limits: { lodCompactBelow: number; lodMiniBelow: number }): Lod {
  return zoom < limits.lodMiniBelow ? 'mini' : zoom < limits.lodCompactBelow ? 'compact' : 'full'
}
