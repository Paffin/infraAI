/**
 * The keyboard walk over the diagram, as geometry: which card an arrow key goes to, where the Tab key enters, where the focus goes when a card is gone.
 * No DOM here: the canvas feeds in the boxes of the cards as they are on the screen (so a zoomed view, a zone nested in a zone and a proposal being
 * previewed all work the same), and `editor/canvasFocus.ts` does the focusing.
 */

export interface NavBox {
  id: string
  x: number
  y: number
  w: number
  h: number
}

export type Dir = 'up' | 'down' | 'left' | 'right'

const cx = (b: NavBox) => b.x + b.w / 2
const cy = (b: NavBox) => b.y + b.h / 2
const byId = (a: NavBox, b: NavBox) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0)

/**
 * How far off the line a candidate may be before it costs more than the same distance along it (going right, a card one row up and 200 px ahead is a worse
 * choice than the one in the row 300 px ahead). 2 is the usual weight of directional focus: a straight line wins over a shortcut.
 */
const ACROSS_WEIGHT = 2

/**
 * The boxes as a table: rows from the top, each row from the left. A box is in the row of the first box (the one whose centre is highest) when its centre is
 * less than half a height of that first box lower, so a card a few pixels off its neighbour is still in its row. Every box of the next row is lower
 * than every box of this one. The rows do not depend on the order of the input.
 */
export function rowsOf<T extends NavBox>(items: readonly T[]): T[][] {
  const sorted = [...items].sort((a, b) => cy(a) - cy(b) || a.x - b.x || byId(a, b))
  const rows: T[][] = []
  for (const b of sorted) {
    const row = rows[rows.length - 1]
    if (row && cy(b) - cy(row[0]) < row[0].h / 2) row.push(b)
    else rows.push([b])
  }
  return rows.map((row) => row.sort((a, b) => a.x - b.x || byId(a, b)))
}

/** The boxes in reading order: the rows of `rowsOf`, one after the other. */
export const readingOrder = <T extends NavBox>(items: readonly T[]): T[] => rowsOf(items).flat()

/**
 * The box an arrow key goes to from `fromId`. The diagram is walked as a table, so that every box can be reached from every other one:
 *  - Left and Right go along the row, to the next box on that side;
 *  - Up and Down go to the next row on that side, to the box in it whose centre is closest to this one's;
 *  - at the end of a row, Left and Right look for the closest box on that side in the other rows (the least `along + 2 · across`), so that a column
 *    drawn a little lower than its neighbour is not a wall.
 * Equal distances go to the smaller id: the walk is the same every time. `null` at the edge of the diagram, or when `fromId` is not among the boxes.
 */
export function nextInDirection(items: readonly NavBox[], fromId: string, dir: Dir): string | null {
  const rows = rowsOf(items)
  const at = rows.findIndex((row) => row.some((b) => b.id === fromId))
  if (at < 0) return null
  const from = rows[at].find((b) => b.id === fromId)!
  if (dir === 'up' || dir === 'down') {
    const row = rows[at + (dir === 'down' ? 1 : -1)]
    if (!row) return null
    let best: NavBox | null = null
    for (const b of row) {
      const off = Math.abs(cx(b) - cx(from))
      if (!best || off < Math.abs(cx(best) - cx(from)) || (off === Math.abs(cx(best) - cx(from)) && byId(b, best) < 0)) best = b
    }
    return best?.id ?? null
  }
  const sign = dir === 'right' ? 1 : -1
  let inRow: NavBox | null = null
  for (const b of rows[at]) {
    const along = (cx(b) - cx(from)) * sign
    if (along > 0 && (!inRow || along < (cx(inRow) - cx(from)) * sign)) inRow = b
  }
  if (inRow) return inRow.id
  let best: { b: NavBox; score: number } | null = null
  for (const b of items) {
    const along = (cx(b) - cx(from)) * sign
    if (along <= 0) continue
    const score = along + ACROSS_WEIGHT * Math.abs(cy(b) - cy(from))
    if (!best || score < best.score || (score === best.score && byId(b, best.b) < 0)) best = { b, score }
  }
  return best?.b.id ?? null
}

/** The box closest to `fromId` by the distance of the centres, leaving out `exclude` (what is about to be deleted); `null` when nothing is left. */
export function nearestTo(items: readonly NavBox[], fromId: string, exclude: ReadonlySet<string> = new Set()): string | null {
  const from = items.find((b) => b.id === fromId)
  if (!from) return null
  let best: { b: NavBox; d: number } | null = null
  for (const b of items) {
    if (b.id === fromId || exclude.has(b.id)) continue
    const d = Math.hypot(cx(b) - cx(from), cy(b) - cy(from))
    if (!best || d < best.d || (d === best.d && byId(b, best.b) < 0)) best = { b, d }
  }
  return best?.b.id ?? null
}

/** The ids with `id` added, or taken out when it is there: what Shift+Enter does to the selection. */
export function toggleId(ids: readonly string[], id: string): string[] {
  return ids.includes(id) ? ids.filter((x) => x !== id) : [...ids, id]
}

/**
 * The smallest move of a view that brings `box` inside `area` with `gap` to spare on each side (all in the same coordinates): `{ dx, dy }` to add to
 * the box (so the view moves by the same amount). A box larger than the area is put against its top-left corner. `{ 0, 0 }` when it is inside already.
 */
export function moveIntoArea(box: { left: number; top: number; right: number; bottom: number }, area: { left: number; top: number; right: number; bottom: number }, gap: number): { dx: number; dy: number } {
  const axis = (lo: number, hi: number, aLo: number, aHi: number) => {
    const from = aLo + gap
    const to = aHi - gap
    if (hi - lo >= to - from) return from - lo // larger than the room: its start against the start of the room
    if (lo < from) return from - lo
    if (hi > to) return to - hi
    return 0
  }
  return { dx: axis(box.left, box.right, area.left, area.right), dy: axis(box.top, box.bottom, area.top, area.bottom) }
}
