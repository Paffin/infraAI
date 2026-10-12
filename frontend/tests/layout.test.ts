import { readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import type { Architecture, Catalog } from '../src/api/types'
import { indexCatalog } from '../src/lib/catalog'
import { CARD_H, CARD_W, computeDepths, hasPos, hasRect, layoutAll, mergeArchitecture, nodeRect, placeMissing, zoneAt, zoneRect } from '../src/editor/layout'

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
