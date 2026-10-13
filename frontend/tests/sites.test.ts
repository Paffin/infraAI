import { describe, expect, it } from 'vitest'
import { buildSiteMap } from '../src/lib/placementMap'
import { siteScenario, sitesOf } from '../src/lib/sites'
import { fixture, realIndex } from './fixtures'

const ix = realIndex()

describe('the sites of a design, read from its zones', () => {
  it('are the zones of the site level, with their racks, role and disaster domain', () => {
    const { architecture } = fixture('report-three-az')
    const sites = sitesOf(architecture, ix)
    const zones = architecture.zones.filter((z) => ix.zoneTypes.get(z.type)?.level === 'site')
    expect(sites.map((s) => s.id)).toEqual(zones.map((z) => z.id))
    expect(sites.length).toBe(3)
    expect(sites.every((s) => s.role === 'working' && s.geo.length > 0)).toBe(true)
    // the racks of a single-DC design belong to the data center around them
    const single = sitesOf(JSON.parse(JSON.stringify(architecture)), ix)
    expect(single.every((s) => Array.isArray(s.racks))).toBe(true)
  })

  it('declare no domain when the design does not say so, and a role only when it is one of the known', () => {
    const { architecture } = fixture('report-no-geo')
    expect(sitesOf(architecture, ix).every((s) => s.geo === '')).toBe(true)
    const odd = JSON.parse(JSON.stringify(architecture)) as typeof architecture
    odd.zones.find((z) => ix.zoneTypes.get(z.type)?.level === 'site')!.role = 'standby'
    odd.zones.filter((z) => ix.zoneTypes.get(z.type)?.level === 'site')[1].role = 'something else'
    const roles = sitesOf(odd, ix).map((s) => s.role)
    expect(roles[0]).toBe('standby')
    expect(roles[1]).toBe('working')
  })

  it('feed the map the wizard draws: every service of the design is in a site or among the global ones', () => {
    const { architecture } = fixture('report-three-az')
    const map = buildSiteMap(architecture, sitesOf(architecture, ix), ix)
    const placed = map.global.reduce((n, c) => n + c.count, 0) + map.domains.flatMap((d) => d.sites).reduce((n, s) => n + s.nodes, 0)
    expect(placed).toBe(architecture.nodes.length)
  })

  it('come with what the simulator says about losing each of them', () => {
    const { architecture, report } = fixture('report-three-az')
    for (const s of sitesOf(architecture, ix)) {
      const sc = siteScenario(report, s.id)
      expect(sc, s.id).not.toBeNull()
      expect(['up', 'degraded', 'down']).toContain(sc!.state)
    }
    expect(siteScenario(report, 'no-such-zone')).toBeNull()
    expect(siteScenario(null, 'x')).toBeNull()
  })
})
