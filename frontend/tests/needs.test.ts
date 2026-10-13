import { describe, expect, it } from 'vitest'
import { setLang } from '../src/i18n'
import { needDef, needExists, needView, uniqueNeeds } from '../src/lib/needs'
import { fixture, realIndex } from './fixtures'

const ix = realIndex()
const FIXTURES = ['report-three-az', 'report-no-requirements', 'report-overloaded', 'report-gaps', 'report-no-geo', 'report-spof']

describe('needs: the fields that turn a "no data" finding into a verified one', () => {
  it('every need the engine emits points at a field the UI can edit', () => {
    let seen = 0
    for (const name of FIXTURES) {
      const { architecture, report } = fixture(name)
      for (const f of report.findings) {
        for (const need of f.needs ?? []) {
          seen++
          expect(f.status, `${name}/${f.id}: only "no data" findings ask for data`).toBe('no_data')
          expect(needExists(need, architecture), `${name}/${f.id}: ${JSON.stringify(need)} exists`).toBe(true)
          if (need.target !== 'requirements') expect(needDef(need, architecture, ix), `${name}/${f.id}: ${JSON.stringify(need)} is a catalog property`).toBeDefined()
        }
      }
    }
    expect(seen).toBeGreaterThan(10)
  })

  it('every "no data" finding of a missing requirement or property carries a need', () => {
    const { report } = fixture('report-no-requirements')
    const nodata = report.findings.filter((f) => f.status === 'no_data')
    expect(nodata.length).toBeGreaterThanOrEqual(5)
    expect(nodata.filter((f) => !f.needs?.length).map((f) => f.id)).toEqual([])
    expect(new Set(nodata.flatMap((f) => (f.needs ?? []).map((n) => n.prop)))).toEqual(new Set(['rpsPeak', 'budgetMonthly', 'rpoSeconds', 'rtoSeconds']))
  })

  it('describes a requirement, an edge property and a zone property in words', () => {
    setLang('en')
    const nr = fixture('report-no-requirements')
    const rto = nr.report.findings.flatMap((f) => f.needs ?? []).find((n) => n.prop === 'rtoSeconds')!
    expect(needView(rto, nr.architecture, ix)).toEqual({ prop: 'RTO (recovery time)', where: 'Requirements' })

    const gaps = fixture('report-gaps')
    const lag = gaps.report.findings.flatMap((f) => f.needs ?? []).find((n) => n.target === 'edge')!
    const edge = gaps.architecture.edges.find((e) => e.id === lag.id)!
    const names = Object.fromEntries(gaps.architecture.nodes.map((n) => [n.id, n.name]))
    expect(needView(lag, gaps.architecture, ix)).toEqual({ prop: 'Replication lag', where: `${names[edge.source]} → ${names[edge.target]}` })

    const geo = fixture('report-no-geo')
    const zoneNeed = geo.report.findings.flatMap((f) => f.needs ?? []).find((n) => n.target === 'zone' && n.id === 'dc1')!
    expect(needView(zoneNeed, geo.architecture, ix)).toEqual({ prop: 'Geo domain (city/metro)', where: geo.architecture.zones.find((z) => z.id === 'dc1')!.name })
    setLang('ru')
  })

  it('knows when the element is gone and offers one button per distinct need', () => {
    const geo = fixture('report-no-geo')
    const all = geo.report.findings.flatMap((f) => f.needs ?? [])
    expect(all.length).toBeGreaterThan(new Set(all.map((n) => `${n.id}:${n.prop}`)).size)
    expect(uniqueNeeds(all)).toHaveLength(new Set(all.map((n) => `${n.id}:${n.prop}`)).size)
    const need = all.find((n) => n.id === 'dc1')!
    const without = { ...geo.architecture, zones: geo.architecture.zones.filter((z) => z.id !== 'dc1') }
    expect(needExists(need, geo.architecture)).toBe(true)
    expect(needExists(need, without)).toBe(false)
    expect(needExists({ target: 'requirements', prop: 'rtoSeconds' }, without)).toBe(true)
  })

  it('the availability-zone type accepts the geo label the engine asks for', () => {
    // regression: `az` zones had no geo property, so a "no data" finding about their geo domain led nowhere
    const az = ix.zoneTypes.get('az')!
    expect(az.props.map((p) => p.key)).toContain('geo')
  })
})
