import { readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import type { Architecture, Catalog } from '../src/api/types'
import { CARD_H, CARD_W, layoutAll, zoneRect } from '../src/editor/layout'
import { indexCatalog } from '../src/lib/catalog'
import { moveIntoArea, nearestTo, nextInDirection, readingOrder, rowsOf, toggleId, type Dir, type NavBox } from '../src/lib/canvasNav'

// The walk is judged on the reference architectures the product ships (laid out by the product's own layout), not on look-alikes.
const defaults = join(process.cwd(), '../backend/crates/engine/defaults')
const ix = indexCatalog(JSON.parse(readFileSync(join(defaults, 'catalog.json'), 'utf8')) as Catalog)
const templates = readdirSync(join(defaults, 'templates')).map((f) => JSON.parse(readFileSync(join(defaults, 'templates', f), 'utf8')) as { id: string; architecture: Architecture })

/** Zone heads are about this large on the screen (a pill at the top-left corner of the zone). */
const HEAD = { dx: 14, dy: 12, w: 180, h: 30 }

/** What the canvas hands to the walk: the cards, and the head of every zone, with the ids the DOM gives them. */
function boxesOf(arch: Architecture): { cards: NavBox[]; all: NavBox[] } {
  const laid = layoutAll(arch, ix)
  const cards = laid.nodes.map((n) => ({ id: 'node:' + n.id, x: n.ui!.x!, y: n.ui!.y!, w: CARD_W, h: CARD_H }))
  const heads = laid.zones.map((z) => {
    const r = zoneRect(z)!
    return { id: 'zone:' + z.id, x: r.x + HEAD.dx, y: r.y + HEAD.dy, w: HEAD.w, h: HEAD.h }
  })
  return { cards, all: [...cards, ...heads] }
}

const DIRS: Dir[] = ['up', 'down', 'left', 'right']
const cx = (b: NavBox) => b.x + b.w / 2
const cy = (b: NavBox) => b.y + b.h / 2

/** A fixed shuffle (Park-Miller), so that a failure can be reproduced. */
function shuffled<T>(list: readonly T[], seed: number): T[] {
  const out = [...list]
  let s = seed
  for (let i = out.length - 1; i > 0; i--) {
    s = (s * 16807) % 2147483647
    const j = s % (i + 1)
    ;[out[i], out[j]] = [out[j], out[i]]
  }
  return out
}

describe('the arrow keys over the reference architectures', () => {
  for (const t of templates) {
    describe(t.id, () => {
      const { cards, all } = boxesOf(t.architecture)
      const byId = new Map(all.map((b) => [b.id, b]))

      it('goes to a box strictly ahead in the direction of the key, never to the box it stands on, the same way every time', () => {
        for (const from of all) {
          for (const dir of DIRS) {
            const to = nextInDirection(all, from.id, dir)
            expect(nextInDirection(all, from.id, dir), `${from.id} ${dir}`).toBe(to)
            if (to === null) continue
            expect(to, `${from.id} ${dir}`).not.toBe(from.id)
            const b = byId.get(to)!
            const ahead = dir === 'right' ? cx(b) - cx(from) : dir === 'left' ? cx(from) - cx(b) : dir === 'down' ? cy(b) - cy(from) : cy(from) - cy(b)
            expect(ahead, `${from.id} ${dir} -> ${to}`).toBeGreaterThan(0)
          }
        }
      })

      it('does not depend on the order in which the boxes are handed over', () => {
        const mixed = shuffled(all, 7)
        for (const from of all) for (const dir of DIRS) expect(nextInDirection(mixed, from.id, dir), `${from.id} ${dir}`).toBe(nextInDirection(all, from.id, dir))
      })

      it('reaches every card and every zone head from the first card in reading order with the four arrows alone', () => {
        const first = readingOrder(cards)[0]
        const seen = new Set([first.id])
        const queue = [first.id]
        while (queue.length) {
          const at = queue.shift()!
          for (const dir of DIRS) {
            const to = nextInDirection(all, at, dir)
            if (to && !seen.has(to)) { seen.add(to); queue.push(to) }
          }
        }
        expect(all.filter((b) => !seen.has(b.id)).map((b) => b.id)).toEqual([])
      })

      it('puts the cards in reading order: rows from the top, each from the left, whatever order they come in', () => {
        const order = readingOrder(cards)
        expect(order.map((b) => b.id)).toEqual(readingOrder(shuffled(cards, 11)).map((b) => b.id))
        expect(order.map((b) => b.id)).toEqual(readingOrder(shuffled(cards, 12345)).map((b) => b.id))
        expect(new Set(order.map((b) => b.id)).size).toBe(cards.length)
        // the rows follow each other downwards, and inside a row the boxes follow each other to the right
        const rows = rowsOf(cards)
        for (let i = 1; i < rows.length; i++) for (const a of rows[i - 1]) for (const b of rows[i]) expect(cy(b), `${a.id} / ${b.id}`).toBeGreaterThan(cy(a))
        for (const row of rows) for (let i = 1; i < row.length; i++) expect(row[i].x, row[i].id).toBeGreaterThanOrEqual(row[i - 1].x)
      })

      it('finds the closest box that stays when a card goes, and none when everything goes', () => {
        const victim = cards[Math.floor(cards.length / 2)]
        const next = nearestTo(all, victim.id, new Set([victim.id]))
        expect(next).not.toBeNull()
        expect(next).not.toBe(victim.id)
        const d = (b: NavBox) => Math.hypot(cx(b) - cx(victim), cy(b) - cy(victim))
        for (const b of all) if (b.id !== victim.id) expect(d(byId.get(next!)!), b.id).toBeLessThanOrEqual(d(b))
        expect(nearestTo(all, victim.id, new Set(all.map((b) => b.id)))).toBeNull()
      })
    })
  }

  it('knows nothing about a box that is not there', () => {
    const { all } = boxesOf(templates[0].architecture)
    expect(nextInDirection(all, 'node:missing', 'right')).toBeNull()
    expect(nearestTo(all, 'node:missing')).toBeNull()
    expect(nextInDirection([], 'node:a', 'down')).toBeNull()
    expect(readingOrder([])).toEqual([])
  })
})

describe('what a card does to the selection and to the view', () => {
  it('toggles an id in a list', () => {
    expect(toggleId(['a', 'b'], 'c')).toEqual(['a', 'b', 'c'])
    expect(toggleId(['a', 'b'], 'a')).toEqual(['b'])
    expect(toggleId([], 'a')).toEqual(['a'])
    expect(toggleId(['a'], 'a')).toEqual([])
  })

  const area = { left: 100, top: 50, right: 900, bottom: 650 }
  const box = (left: number, top: number, w = 212, h = 64) => ({ left, top, right: left + w, bottom: top + h })

  it('leaves a box that is inside the room where it is', () => {
    expect(moveIntoArea(box(300, 300), area, 16)).toEqual({ dx: 0, dy: 0 })
    expect(moveIntoArea(box(116, 66), area, 16)).toEqual({ dx: 0, dy: 0 })
  })

  it('moves the least that brings a box inside, with the gap to spare', () => {
    expect(moveIntoArea(box(20, 300), area, 16)).toEqual({ dx: 96, dy: 0 }) // 20 -> 116
    expect(moveIntoArea(box(800, 300), area, 16)).toEqual({ dx: -128, dy: 0 }) // right edge 1012 -> 884
    expect(moveIntoArea(box(300, 0), area, 16)).toEqual({ dx: 0, dy: 66 })
    expect(moveIntoArea(box(300, 640), area, 16)).toEqual({ dx: 0, dy: -70 })
    expect(moveIntoArea(box(-50, 700), area, 16)).toEqual({ dx: 166, dy: -130 })
  })

  it('puts a box that is larger than the room against its top-left corner', () => {
    expect(moveIntoArea(box(0, 0, 2000, 2000), area, 16)).toEqual({ dx: 116, dy: 66 })
  })
})
