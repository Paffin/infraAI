// Where a label sits on a link. The middle of the link is where it reads best, but a link often passes behind other cards (a replication
// stream between two sites crosses the services in between), and a label in the middle would hide a name. The spot is the nearest to the
// middle that is clear of every card.

export interface Box { x: number; y: number; w: number; h: number }
export interface Pt { x: number; y: number }
/** A spot for a label: `clear` says it stays clear of every card (a link that runs along a row of cards has none). */
export interface Spot extends Pt { clear: boolean }

/** The fractions of the link's length to try, the middle first and then outwards. */
export const LABEL_FRACTIONS = [0.5, 0.42, 0.58, 0.34, 0.66, 0.26, 0.74, 0.18, 0.82]

const overlap = (a: Box, b: Box) => a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y

/**
 * The point of the link for a label of `size` (centred on the point): the first of `LABEL_FRACTIONS` whose label stays `margin` clear of the
 * `obstacles`. When none does (a short link between two cards, a link along a row of cards) the spot is the middle and not `clear`: a label there
 * would sit on a card, so the drawing shows it only when asked for (hover, selection).
 */
export function pickLabelSpot(at: (fraction: number) => Pt, size: { w: number; h: number }, obstacles: Box[], margin = 5): Spot {
  for (const f of LABEL_FRACTIONS) {
    const p = at(f)
    const box: Box = { x: p.x - size.w / 2 - margin, y: p.y - size.h / 2 - margin, w: size.w + 2 * margin, h: size.h + 2 * margin }
    if (!obstacles.some((o) => overlap(box, o))) return { ...p, clear: true }
  }
  return { ...at(LABEL_FRACTIONS[0]), clear: false }
}

/** A label's size on the canvas, near enough to keep clear by: 6.4 px a character and the padding. */
export const labelSize = (text: string) => ({ w: Math.ceil(text.length * 6.4) + 22, h: 22 })
