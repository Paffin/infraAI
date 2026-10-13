import type { Architecture, PlacedSite, Report, ScenarioSummary, Zone } from '@/api/types'
import type { CatalogIndex } from './catalog'

/** Roles a zone can declare for the site it is (`active`, `standby`, `witness`, `backup`); anything else is a working site. */
const ROLES: Record<string, PlacedSite['role']> = { active: 'working', working: 'working', standby: 'standby', witness: 'witness', backup: 'backup' }

/** The disaster domain a site declares (`geo`), or the one of the zone around it. */
function geoOf(z: Zone, byId: Map<string, Zone>): string {
  for (let cur: Zone | undefined = z, hops = 0; cur && hops < 32; cur = cur.parentId ? byId.get(cur.parentId) : undefined, hops++) {
    const g = cur.props.geo
    if (typeof g === 'string' && g) return g
  }
  return ''
}

/** The sites of a design: its zones of the site level, with the racks inside them, the role and the disaster domain they declare. */
export function sitesOf(arch: Architecture, ix: CatalogIndex): PlacedSite[] {
  const level = (z: Zone) => ix.zoneTypes.get(z.type)?.level
  const byId = new Map(arch.zones.map((z) => [z.id, z]))
  return arch.zones
    .filter((z) => level(z) === 'site')
    .map((z) => ({
      id: z.id,
      name: z.name,
      role: ROLES[(z.role ?? '').toLowerCase()] ?? 'working',
      geo: geoOf(z, byId),
      racks: arch.zones.filter((r) => r.parentId === z.id && level(r) === 'rack').map((r) => r.id),
    }))
}

/** What the simulator says about losing that site (null: the report has no such scenario). */
export function siteScenario(report: Pick<Report, 'scenarios'> | null | undefined, zoneId: string): ScenarioSummary | null {
  return report?.scenarios.find((s) => s.scenario.type === 'zone' && s.scenario.zoneId === zoneId) ?? null
}
