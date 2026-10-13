import { describe, expect, it } from 'vitest'
import type { Architecture, PlacedSite } from '../src/api/types'
import { indexCatalog } from '../src/lib/catalog'
import { buildSiteMap, comparisonSites } from '../src/lib/placementMap'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

const catalog = JSON.parse(readFileSync(join(__dirname, '../../backend/crates/engine/defaults/catalog.json'), 'utf8'))
const ix = indexCatalog(catalog)
const node = (id: string, kind: string, zoneId: string | null, role: string | null = null, group: string | null = kind) => ({ id, kind, name: id, zoneId, group, role, props: {} })

describe('site map', () => {
  const arch: Architecture = {
    schemaVersion: 1, meta: {}, requirements: {},
    zones: [
      { id: 'dc1', name: 'DC-1', type: 'dc', parentId: null, role: null, props: {} },
      { id: 'dc1-r1', name: 'DC-1 · rack 1', type: 'rack', parentId: 'dc1', role: null, props: {} },
      { id: 'dc2', name: 'DC-2', type: 'dc', parentId: null, role: null, props: {} },
      { id: 'w1', name: 'Arbiter', type: 'cloud', parentId: null, role: null, props: {} },
    ],
    nodes: [
      node('users', 'users', null, null, null),
      node('db-dc1', 'postgresql', 'dc1', 'primary', 'db'),
      node('app-r1', 'kubernetes', 'dc1-r1', null, 'app'),
      node('db-dc2', 'postgresql', 'dc2', 'replica', 'db'),
      node('db-w', 'postgresql', 'w1', 'witness', 'db'),
      node('etcd-1', 'etcd', 'dc1', 'voter', 'etcd'),
      node('etcd-2', 'etcd', 'dc1', 'voter', 'etcd'),
    ],
    edges: [],
  }
  const sites: PlacedSite[] = [
    { id: 'dc1', name: 'DC-1', role: 'working', geo: 'geo-1', racks: ['dc1-r1'] },
    { id: 'dc2', name: 'DC-2', role: 'working', geo: 'geo-1', racks: [] },
    { id: 'w1', name: 'Arbiter', role: 'witness', geo: 'geo-2', racks: [] },
  ]

  it('shows what runs where, grouped by disaster domain', () => {
    const m = buildSiteMap(arch, sites, ix)
    expect(m.global.map((c) => c.kind)).toEqual(['users'])
    expect(m.domains.map((d) => [d.geo, d.sites.map((s) => s.id)])).toEqual([['geo-1', ['dc1', 'dc2']], ['geo-2', ['w1']]])
    const dc1 = m.domains[0].sites[0]
    expect(dc1.nodes).toBe(4)
    expect(dc1.racks[0].chips.map((c) => c.kind)).toEqual(['kubernetes'])
    // two voters of one cluster in a place are one chip with a count
    expect(dc1.chips.find((c) => c.kind === 'etcd')?.count).toBe(2)
    expect(m.domains[1].sites[0].chips[0]).toMatchObject({ kind: 'postgresql', role: 'witness' })
  })
})

describe('comparison sizes', () => {
  it('covers the corners and round numbers of the range', () => {
    expect(comparisonSites(1, 1, 12)).toEqual([1])
    expect(comparisonSites(3, 20, 12)).toEqual([3, 4, 5, 7, 10, 15, 20])
    expect(comparisonSites(2, 2, 12)).toEqual([2])
  })
  it('thins out an over-long list but keeps both ends', () => {
    const r = comparisonSites(1, 20, 4)
    expect(r.length).toBeLessThanOrEqual(4)
    expect(r[0]).toBe(1)
    expect(r[r.length - 1]).toBe(20)
  })
})
