/**
 * Level of detail of the canvas by zoom ("semantic zoom"). Zoomed out, a card drops its subtitle and keeps its
 * name at a legible screen size (`compact`); further out it becomes a coloured tile (`mini`) – the overview of a
 * big architecture stays readable instead of turning into grey specks.
 */
export type Lod = 'full' | 'compact' | 'mini'

export const LOD_COMPACT_BELOW = 0.62
export const LOD_MINI_BELOW = 0.3

export function lodOf(zoom: number): Lod {
  return zoom < LOD_MINI_BELOW ? 'mini' : zoom < LOD_COMPACT_BELOW ? 'compact' : 'full'
}
