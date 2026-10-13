import { readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import type { Architecture, Catalog } from '../src/api/types'
import { indexCatalog } from '../src/lib/catalog'
import { CARD_H, CARD_W, computeDepths, hasPos, hasRect, layoutAll, mergeArchitecture, nodeRect, placeInZone, placeMissing, structureChanged, structureDrift, zoneAt, zoneRect } from '../src/editor/layout'

const defaults = join(process.cwd(), '../backend/crates/engine/defaults')
const catalog = JSON.parse(readFileSync(join(defaults, 'catalog.json'), 'utf8')) as Catalog
const ix = indexCatalog(catalog)
const templates = readdirSync(join(defaults, 'templates')).map((f) => JSON.parse(readFileSync(join(defaults, 'templates', f), 'utf8')) as { id: string; architecture: Architecture })

const overlap = (a: { x: number; y: number; w: number; h: number }, b: { x: number; y: number; w: number; h: number }) =>
  a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y

describe('layoutAll', () => {
  for (const t of templates) {
    describe(t.id, () => {
      const laid = layoutAll(t.architecture, ix)

      it('positions every node and zone', () => {
        expect(laid.nodes.every((n) => hasPos(n.ui))).toBe(true)
        expect(laid.zones.every((z) => hasRect(z.ui))).toBe(true)
      })

      it('never overlaps two cards', () => {
        const rects = laid.nodes.map((n) => nodeRect(n)!)
        for (let i = 0; i < rects.length; i++) for (let j = i + 1; j < rects.length; j++) expect(overlap(rects[i], rects[j])).toBe(false)
      })

      it('keeps every node inside its zone and every zone inside its parent', () => {
        const zones = new Map(laid.zones.map((z) => [z.id, zoneRect(z)!]))
        for (const n of laid.nodes) {
          if (!n.zoneId) continue
          const r = nodeRect(n)!
          const z = zones.get(n.zoneId)!
          expect(r.x >= z.x && r.y >= z.y && r.x + r.w <= z.x + z.w && r.y + r.h <= z.y + z.h, `${n.id} in ${n.zoneId}`).toBe(true)
        }
        for (const z of laid.zones) {
          if (!z.parentId) continue
          const p = zones.get(z.parentId)!
          const r = zones.get(z.id)!
          expect(r.x >= p.x && r.y >= p.y && r.x + r.w <= p.x + p.w && r.y + r.h <= p.y + p.h, `${z.id} in ${z.parentId}`).toBe(true)
        }
      })

      it('does not let sibling zones or global nodes cut into zones', () => {
        const tops = laid.zones.filter((z) => !z.parentId).map((z) => zoneRect(z)!)
        for (let i = 0; i < tops.length; i++) for (let j = i + 1; j < tops.length; j++) expect(overlap(tops[i], tops[j])).toBe(false)
        for (const n of laid.nodes.filter((n) => !n.zoneId)) for (const z of tops) expect(overlap(nodeRect(n)!, z), n.id).toBe(false)
      })

      it('is deterministic and does not mutate its input', () => {
        const before = JSON.stringify(t.architecture)
        const again = layoutAll(t.architecture, ix)
        expect(JSON.stringify(again)).toBe(JSON.stringify(laid))
        expect(JSON.stringify(t.architecture)).toBe(before)
      })
    })
  }

  it('aligns equivalent nodes of different sites on one row', () => {
    const t = templates.find((x) => x.id === 'two-dc-witness')!
    const laid = layoutAll(t.architecture, ix)
    const y = (id: string) => laid.nodes.find((n) => n.id === id)!.ui!.y
    expect(y('app-dc1')).toBe(y('app-dc2'))
    expect(y('ingress-dc1')).toBe(y('ingress-dc2'))
    expect(y('pg-primary')).toBe(y('pg-replica') ?? y('pg-primary'))
  })

  it('puts entry points above the sites', () => {
    const t = templates.find((x) => x.id === 'two-dc-witness')!
    const laid = layoutAll(t.architecture, ix)
    const users = laid.nodes.find((n) => n.id === 'users')!
    const zoneTop = Math.min(...laid.zones.map((z) => z.ui!.y!))
    expect(users.ui!.y! + CARD_H).toBeLessThan(zoneTop)
  })

  it('survives cycles and unknown references', () => {
    const arch: Architecture = {
      schemaVersion: 1, meta: {}, requirements: {}, zones: [{ id: 'z', name: 'Z', type: 'dc', parentId: 'nope', role: null, props: {} }],
      nodes: [
        { id: 'a', kind: 'kubernetes', name: 'a', zoneId: 'z', group: null, role: null, props: {} },
        { id: 'b', kind: 'postgresql', name: 'b', zoneId: 'ghost', group: null, role: null, props: {} },
      ],
      edges: [
        { id: 'e1', source: 'a', target: 'b', type: 'traffic', label: null, props: {} },
        { id: 'e2', source: 'b', target: 'a', type: 'traffic', label: null, props: {} },
      ],
    }
    const laid = layoutAll(arch, ix)
    expect(laid.nodes.every((n) => hasPos(n.ui))).toBe(true)
    expect([...computeDepths(arch, ix).values()].every((d) => d !== null)).toBe(true)
  })
})

describe('placeMissing', () => {
  const base = layoutAll(templates.find((x) => x.id === 'two-dc-witness')!.architecture, ix)

  it('keeps existing positions and puts a new node inside its zone without overlap', () => {
    const arch = JSON.parse(JSON.stringify(base)) as Architecture
    arch.nodes.push({ id: 'new', kind: 'redis', name: 'Redis', zoneId: 'dc1', group: null, role: null, props: {} })
    const out = placeMissing(arch, ix)
    expect(out.nodes.slice(0, -1).map((n) => n.ui)).toEqual(base.nodes.map((n) => n.ui))
    const added = out.nodes.find((n) => n.id === 'new')!
    const z = out.zones.find((z) => z.id === 'dc1')!
    expect(added.ui!.x! >= z.ui!.x! && added.ui!.x! + CARD_W <= z.ui!.x! + z.ui!.w!).toBe(true)
    expect(added.ui!.y! >= z.ui!.y! && added.ui!.y! + CARD_H <= z.ui!.y! + z.ui!.h!).toBe(true)
    for (const n of out.nodes) if (n.id !== 'new') expect(overlap(nodeRect(n)!, nodeRect(added)!)).toBe(false)
  })

  it('lays out everything when nothing has coordinates', () => {
    const out = placeMissing(templates[0].architecture, ix)
    expect(out.nodes.every((n) => hasPos(n.ui))).toBe(true)
  })

  it('is a no-op when everything is placed', () => {
    expect(placeMissing(base, ix)).toBe(base)
  })
})

describe('queries and merge', () => {
  const base = layoutAll(templates.find((x) => x.id === 'two-dc-witness')!.architecture, ix)

  it('finds the innermost zone under a point', () => {
    const z = base.zones.find((z) => z.id === 'dc2')!
    expect(zoneAt(base, z.ui!.x! + 20, z.ui!.y! + 20)?.id).toBe('dc2')
    expect(zoneAt(base, -9999, -9999)).toBeNull()
  })

  it('merges a second architecture side by side with fresh ids', () => {
    const add = templates.find((x) => x.id === 'two-dc-witness')!.architecture
    const merged = mergeArchitecture(base, add, ix)
    expect(merged.nodes.length).toBe(base.nodes.length * 2)
    expect(new Set(merged.nodes.map((n) => n.id)).size).toBe(merged.nodes.length)
    expect(new Set(merged.zones.map((z) => z.id)).size).toBe(merged.zones.length)
    const ids = new Set(merged.nodes.map((n) => n.id))
    expect(merged.edges.every((e) => ids.has(e.source) && ids.has(e.target))).toBe(true)
    expect(merged.nodes.every((n) => !n.zoneId || merged.zones.some((z) => z.id === n.zoneId))).toBe(true)
    const maxBase = Math.max(...base.nodes.map((n) => n.ui!.x! + CARD_W))
    expect(Math.min(...merged.nodes.slice(base.nodes.length).map((n) => n.ui!.x!))).toBeGreaterThan(maxBase)
  })
})

describe('placeInZone (move nodes to another zone)', () => {
  const t = templates.find((x) => x.id === 'three-az-region')!
  const laid = layoutAll(t.architecture, ix)
  const inZone = (a: Architecture, zid: string) => a.nodes.filter((n) => n.zoneId === zid)

  it('puts the nodes inside the target zone, on free slots, without touching the others', () => {
    const [from, to] = laid.zones.filter((z) => z.parentId).slice(0, 2)
    const moving = inZone(laid, from.id).slice(0, 3).map((n) => n.id)
    expect(moving).toHaveLength(3)
    const out = placeInZone(laid, moving, to.id)
    const zr = zoneRect(out.zones.find((z) => z.id === to.id)!)!
    for (const id of moving) {
      const n = out.nodes.find((x) => x.id === id)!
      expect(n.zoneId).toBe(to.id)
      const r = nodeRect(n)!
      expect(r.x).toBeGreaterThanOrEqual(zr.x)
      expect(r.y).toBeGreaterThanOrEqual(zr.y)
      expect(r.x + r.w).toBeLessThanOrEqual(zr.x + zr.w)
      expect(r.y + r.h).toBeLessThanOrEqual(zr.y + zr.h)
    }
    // no card on top of another one anywhere
    const rects = out.nodes.map((n) => nodeRect(n)!)
    for (let i = 0; i < rects.length; i++) for (let j = i + 1; j < rects.length; j++) expect(overlap(rects[i], rects[j])).toBe(false)
    // the rest of the diagram is where it was
    for (const n of laid.nodes.filter((x) => !moving.includes(x.id))) expect(out.nodes.find((x) => x.id === n.id)!.ui).toEqual(n.ui)
    // pure: the input is untouched
    expect(laid.nodes.find((n) => n.id === moving[0])!.zoneId).toBe(from.id)
  })

  it('grows the zone when its grid is full', () => {
    const [, to] = laid.zones.filter((z) => z.parentId)
    const before = zoneRect(to)!
    const crowd = laid.nodes.filter((n) => n.zoneId !== to.id).slice(0, 14).map((n) => n.id)
    const out = placeInZone(laid, crowd, to.id)
    const after = zoneRect(out.zones.find((z) => z.id === to.id)!)!
    expect(after.h).toBeGreaterThan(before.h)
    for (const id of crowd) {
      const r = nodeRect(out.nodes.find((n) => n.id === id)!)!
      expect(r.y + r.h).toBeLessThanOrEqual(after.y + after.h)
    }
    const rects = out.nodes.map((n) => nodeRect(n)!)
    for (let i = 0; i < rects.length; i++) for (let j = i + 1; j < rects.length; j++) expect(overlap(rects[i], rects[j])).toBe(false)
  })

  it('leaves the architecture alone when the zone has no rectangle', () => {
    const bare = { ...laid, zones: laid.zones.map((z) => ({ ...z, ui: undefined })) }
    const id = laid.nodes[0].id
    expect(placeInZone(bare, [id], laid.zones[0].id).nodes.find((n) => n.id === id)!.zoneId).toBe(laid.nodes[0].zoneId)
  })
})


// ------------------------------------------------------------------ an answer that rearranges the zones

/** The recorded runs of the chat: "make it fault tolerant within one data center" (tests/fixtures/ai-one-dc-*.json). */
const run = (name: string) => JSON.parse(readFileSync(join(process.cwd(), 'tests', 'fixtures', name), 'utf8')) as { previous: Architecture; next: Architecture }

/** Every card inside the zone it belongs to, every zone inside its parent, no card on another. */
function expectCoherent(a: Architecture) {
  const zones = new Map(a.zones.map((z) => [z.id, zoneRect(z)!]))
  for (const n of a.nodes) {
    const r = nodeRect(n)
    expect(r, `${n.id} has a position`).toBeTruthy()
    if (n.zoneId) {
      const z = zones.get(n.zoneId)!
      expect(z, `zone ${n.zoneId} has a rectangle`).toBeTruthy()
      expect(r!.x >= z.x && r!.y >= z.y && r!.x + r!.w <= z.x + z.w && r!.y + r!.h <= z.y + z.h, `${n.id} lies in ${n.zoneId}`).toBe(true)
    }
  }
  for (const z of a.zones) {
    expect(zones.get(z.id), `zone ${z.id} has a rectangle`).toBeTruthy()
    if (!z.parentId) continue
    const p = zones.get(z.parentId)!
    const r = zones.get(z.id)!
    expect(r.x >= p.x && r.y >= p.y && r.x + r.w <= p.x + p.w && r.y + r.h <= p.y + p.h, `${z.id} is drawn in ${z.parentId}`).toBe(true)
  }
  const rects = a.nodes.map((n) => nodeRect(n)!)
  for (let i = 0; i < rects.length; i++) for (let j = i + 1; j < rects.length; j++) expect(overlap(rects[i], rects[j]), `${a.nodes[i].id} on ${a.nodes[j].id}`).toBe(false)
}

describe('a change that rearranges the failure domains', () => {
  for (const file of ['ai-one-dc-racks.json', 'ai-one-dc-extra-site.json']) {
    describe(file, () => {
      const { previous, next } = run(file)

      it('is a picture that contradicts the model when the new things only get a free place', () => {
        // what the editor used to show: racks to the right of the data center, empty, and their cards still in the old box
        const old = placeMissing(next, ix)
        expect(structureDrift(old)).toBe(true)
        expect(structureChanged(previous, next)).toBe(true)
      })

      it('is drawn laid out again: racks in the data center, every card in its rack', () => {
        const laid = placeMissing(next, ix, { previous })
        expectCoherent(laid)
        // the racks that hold cards are not empty boxes
        for (const z of laid.zones.filter((x) => x.type === 'rack')) expect(laid.nodes.some((n) => n.zoneId === z.id), `${z.id} holds a card`).toBe(true)
        // the data center is the box around them
        const dc = laid.zones.find((z) => z.id === 'dc1')!
        for (const z of laid.zones.filter((x) => x.parentId === dc.id)) expect(z.ui!.w!).toBeLessThan(dc.ui!.w!)
      })

      it('is the same picture every time and one that needs no further layout', () => {
        const once = placeMissing(next, ix, { previous })
        expect(JSON.stringify(placeMissing(next, ix, { previous }))).toBe(JSON.stringify(once))
        expect(placeMissing(once, ix, { previous: once })).toBe(once)
        expect(structureDrift(once)).toBe(false)
      })

      it('keeps the model: nothing but positions changes', () => {
        const laid = placeMissing(next, ix, { previous })
        const strip = (a: Architecture) => JSON.stringify({ ...a, zones: a.zones.map(({ ui: _u, ...z }) => z), nodes: a.nodes.map(({ ui: _u, ...n }) => n) })
        expect(strip(laid)).toBe(strip(next))
      })
    })
  }

  it('leaves the cards where they are when only a card is added to a zone', () => {
    const { previous } = run('ai-one-dc-racks.json')
    const next: Architecture = JSON.parse(JSON.stringify(previous))
    next.nodes.push({ id: 'cache', kind: 'redis', name: 'Cache', zoneId: 'dc1', group: null, role: null, props: {} })
    expect(structureChanged(previous, next)).toBe(false)
    const out = placeMissing(next, ix, { previous })
    for (const n of previous.nodes) expect(out.nodes.find((x) => x.id === n.id)!.ui).toEqual(n.ui)
    expect(hasPos(out.nodes.find((n) => n.id === 'cache')!.ui)).toBe(true)
    expectCoherent(out)
  })

  it('does not take a hand-made arrangement apart when the zones did not change', () => {
    // a zone the user made smaller than its cards: odd, but the user's own picture, not a change of structure
    const { previous } = run('ai-one-dc-racks.json')
    const hand: Architecture = JSON.parse(JSON.stringify(previous))
    hand.zones[0].ui = { ...hand.zones[0].ui, w: 200, h: 200 }
    expect(structureDrift(hand)).toBe(true)
    expect(placeMissing(hand, ix, { previous: hand })).toBe(hand)
    expect(placeMissing(hand, ix)).toBe(hand)
  })

  it('tells what a change of structure is', () => {
    const { previous } = run('ai-one-dc-racks.json')
    const copy = () => JSON.parse(JSON.stringify(previous)) as Architecture
    expect(structureChanged(previous, copy())).toBe(false)
    const moved = copy()
    moved.nodes.find((n) => n.zoneId)!.zoneId = null
    expect(structureChanged(previous, moved)).toBe(true)
    const zone = copy()
    zone.zones.push({ id: 'z2', name: 'Z', type: 'rack', parentId: 'dc1', role: null, props: {} })
    expect(structureChanged(previous, zone)).toBe(true)
    const reparented = copy()
    reparented.zones.push({ id: 'z3', name: 'Z', type: 'rack', parentId: null, role: null, props: {}, ui: { x: 0, y: 0, w: 300, h: 300 } })
    const into = JSON.parse(JSON.stringify(reparented)) as Architecture
    into.zones[into.zones.length - 1].parentId = 'dc1'
    expect(structureChanged(reparented, into)).toBe(true)
  })
})

describe('a zone inside a zone', () => {
  it('is covered by its parent even when it has a single row of cards (a zone is never shorter than its minimum)', () => {
    const arch: Architecture = {
      schemaVersion: 1, meta: {}, requirements: {},
      zones: [
        { id: 'dc', name: 'DC', type: 'dc', parentId: null, role: null, props: {} },
        { id: 'rack', name: 'Rack', type: 'rack', parentId: 'dc', role: null, props: {} },
      ],
      nodes: [
        { id: 'app', kind: 'kubernetes', name: 'App', zoneId: 'rack', group: null, role: null, props: {} },
        { id: 'db', kind: 'postgresql', name: 'DB', zoneId: 'rack', group: null, role: null, props: {} },
      ],
      edges: [],
    }
    const laid = layoutAll(arch, ix)
    const dc = zoneRect(laid.zones[0])!
    const rack = zoneRect(laid.zones[1])!
    expect(rack.x).toBeGreaterThanOrEqual(dc.x)
    expect(rack.y).toBeGreaterThanOrEqual(dc.y)
    expect(rack.x + rack.w).toBeLessThanOrEqual(dc.x + dc.w)
    expect(rack.y + rack.h).toBeLessThanOrEqual(dc.y + dc.h)
    expect(structureDrift(laid)).toBe(false)
  })
})
