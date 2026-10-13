import { describe, expect, it } from 'vitest'
import { LABEL_FRACTIONS, labelSize, pickLabelSpot, type Box } from '../src/lib/labelSpot'

// a straight link from (0, 0) to (400, 0): the point at a fraction is easy to say
const at = (f: number) => ({ x: 400 * f, y: 0 })
const size = { w: 60, h: 22 }

describe('the spot of a label on a link', () => {
  it('is the middle when nothing is in the way', () => {
    expect(pickLabelSpot(at, size, [])).toEqual({ x: 200, y: 0, clear: true })
  })

  it('moves along the link, nearest the middle first, when a card is in the middle', () => {
    const card: Box = { x: 150, y: -32, w: 100, h: 64 }
    const p = pickLabelSpot(at, size, [card])
    expect(p.x).not.toBe(200)
    // the first fraction that clears the card, in the order of the list
    const clear = LABEL_FRACTIONS.find((f) => at(f).x - 30 - 5 >= 250 || at(f).x + 30 + 5 <= 150)!
    expect(p).toEqual({ ...at(clear), clear: true })
    expect(Math.abs(p.x - 200)).toBeLessThan(120)
  })

  it('keeps clear of every card, not just the first', () => {
    const cards: Box[] = [{ x: 120, y: -32, w: 60, h: 64 }, { x: 220, y: -32, w: 60, h: 64 }]
    const p = pickLabelSpot(at, size, cards)
    for (const c of cards) expect(p.x + 30 + 5 <= c.x || p.x - 30 - 5 >= c.x + c.w).toBe(true)
  })

  it('stays in the middle, and says it is not clear, for a link with no room anywhere', () => {
    const wall: Box = { x: -10, y: -40, w: 420, h: 80 }
    expect(pickLabelSpot(at, size, [wall])).toEqual({ x: 200, y: 0, clear: false })
  })

  it('is not troubled by cards that are far from the link', () => {
    expect(pickLabelSpot(at, size, [{ x: 150, y: 80, w: 100, h: 64 }])).toEqual({ x: 200, y: 0, clear: true })
  })

  it('sizes a label by its text', () => {
    expect(labelSize('sync').w).toBeLessThan(labelSize('sync · RTT 1.2 мс').w)
    expect(labelSize('').h).toBe(22)
  })
})
