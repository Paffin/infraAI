import type { Architecture, ArchNode, PlacedSite } from '@/api/types'
import type { CatalogIndex } from './catalog'

/** A service in a place, as a coloured chip: the kind's badge and the role it plays (primary, replica, arbiter…). */
export interface MapChip {
  key: string
  kind: string
  abbr: string
  color: string
  /** group (cluster) the nodes belong to; chips of one group are connected visually by the same ring */
  group: string | null
  role: string | null
  count: number
}

export interface MapPlace {
  /** the zone id */
  id: string
  name: string
  chips: MapChip[]
}

export interface MapSite extends MapPlace {
  role: PlacedSite['role']
  geo: string
  racks: MapPlace[]
  nodes: number
}

export interface SiteMap {
  /** components without a zone: users, DNS, WAF */
  global: MapChip[]
  /** sites grouped by disaster domain, in order of appearance */
  domains: { geo: string; sites: MapSite[] }[]
}

/** Roles worth showing on a chip, as the one letter the chip carries. */
export const ROLE_MARK: Record<string, string> = { primary: 'P', replica: 'R', witness: 'W', voter: 'V' }

function chipsOf(nodes: ArchNode[], ix: CatalogIndex): MapChip[] {
  const out = new Map<string, MapChip>()
  for (const n of nodes) {
    const kind = ix.kinds.get(n.kind)
    const key = `${n.kind}|${n.group ?? ''}|${n.role ?? ''}`
    const cur = out.get(key)
    if (cur) cur.count++
    else out.set(key, { key, kind: n.kind, abbr: kind?.abbr ?? n.kind.slice(0, 3).toUpperCase(), color: kind?.color ?? '#8b93a1', group: n.group, role: n.role, count: 1 })
  }
  return [...out.values()]
}

/** The layout of a generated design for the wizard's map: which services run where. Pure; no coordinates needed. */
export function buildSiteMap(arch: Architecture, sites: PlacedSite[], ix: CatalogIndex): SiteMap {
  const byZone = new Map<string, ArchNode[]>()
  const globals: ArchNode[] = []
  for (const n of arch.nodes) {
    if (!n.zoneId) globals.push(n)
    else {
      if (!byZone.has(n.zoneId)) byZone.set(n.zoneId, [])
      byZone.get(n.zoneId)!.push(n)
    }
  }
  const zoneName = new Map(arch.zones.map((z) => [z.id, z.name]))
  const domains: SiteMap['domains'] = []
  for (const s of sites) {
    const racks: MapPlace[] = s.racks.map((r) => ({ id: r, name: zoneName.get(r) ?? r, chips: chipsOf(byZone.get(r) ?? [], ix) }))
    const own = byZone.get(s.id) ?? []
    const nodes = own.length + racks.reduce((sum, r) => sum + (byZone.get(r.id)?.length ?? 0), 0)
    const site: MapSite = { id: s.id, name: s.name, role: s.role, geo: s.geo, chips: chipsOf(own, ix), racks, nodes }
    let d = domains.find((x) => x.geo === s.geo)
    if (!d) domains.push((d = { geo: s.geo, sites: [] }))
    d.sites.push(site)
  }
  return { global: chipsOf(globals, ix), domains }
}

/** Site counts worth comparing for a pattern: the corners of its range and the round numbers in between. */
export function comparisonSites(min: number, max: number | null | undefined, limit: number, quick: number[] = [2, 3, 4, 5, 7, 10, 15, 20]): number[] {
  const hi = max ?? Math.max(...quick, min)
  const all = [...new Set([min, ...quick.filter((n) => n > min && n < hi), hi])].filter((n) => n >= min && n <= hi).sort((a, b) => a - b)
  if (all.length <= limit) return all
  // keep both ends, thin out the middle evenly
  const keep = [all[0]]
  const mid = all.slice(1, -1)
  const room = Math.max(0, limit - 2)
  for (let i = 0; i < room; i++) keep.push(mid[Math.floor((i * mid.length) / room)])
  keep.push(all[all.length - 1])
  return [...new Set(keep)]
}
