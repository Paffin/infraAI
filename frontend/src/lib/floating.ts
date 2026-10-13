// Where a panel that floats over the canvas sits. The user drags it anywhere; what is kept is not pixels but the fraction of the free room
// (the area minus the panel minus a margin), so that the panel is where it was left, and fully in view, after the window changes size.

export interface Size { w: number; h: number }
export interface Pos { x: number; y: number }
/** A position as the fraction of the free room on each axis (0 = against the left/top margin, 1 = against the right/bottom one). */
export interface Frac { fx: number; fy: number }

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v))

/** The room a panel can move in: how far its top-left may go on each axis (never negative: a panel larger than the area sits at the margin). */
export function room(area: Size, panel: Size, margin: number): Size {
  return { w: Math.max(0, area.w - panel.w - 2 * margin), h: Math.max(0, area.h - panel.h - 2 * margin) }
}

/** The top-left of a panel kept inside the area, `margin` away from its edges. */
export function clampPos(pos: Pos, area: Size, panel: Size, margin: number): Pos {
  const r = room(area, panel, margin)
  return { x: clamp(pos.x, margin, margin + r.w), y: clamp(pos.y, margin, margin + r.h) }
}

/** The fractions of a top-left position. */
export function fracOf(pos: Pos, area: Size, panel: Size, margin: number): Frac {
  const r = room(area, panel, margin)
  return { fx: r.w ? clamp((pos.x - margin) / r.w, 0, 1) : 0, fy: r.h ? clamp((pos.y - margin) / r.h, 0, 1) : 0 }
}

/** The top-left a fraction stands for in an area of this size. */
export function placeFrac(f: Frac, area: Size, panel: Size, margin: number): Pos {
  const r = room(area, panel, margin)
  return { x: Math.round(margin + f.fx * r.w), y: Math.round(margin + f.fy * r.h) }
}

/** A stored position, or null if what was stored is not one (a hand-edited or an older value). */
export function parseFrac(raw: string | null | undefined): Frac | null {
  if (!raw) return null
  try {
    const v = JSON.parse(raw) as Partial<Frac>
    return typeof v.fx === 'number' && typeof v.fy === 'number' && Number.isFinite(v.fx) && Number.isFinite(v.fy) ? { fx: clamp(v.fx, 0, 1), fy: clamp(v.fy, 0, 1) } : null
  } catch {
    return null
  }
}

/** The width of a panel dragged to `x` by its edge, kept between the limits and leaving `keep` px of the container to what is next to it. */
export function clampWidth(width: number, min: number, max: number, container: number, keep: number): number {
  const ceiling = Math.max(min, Math.min(max, container - keep))
  return Math.round(clamp(width, min, ceiling))
}
