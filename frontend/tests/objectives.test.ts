import { describe, expect, it } from 'vitest'
import { objectiveAction, siteScenarios } from '../src/lib/objectives'
import { fixture, ruleCheck } from './fixtures'

const obj = (name: string, id: string) => {
  const f = fixture(name)
  return { ...f, o: f.report.objectives.find((o) => o.id === id)! }
}

describe('objective pills open the evidence behind their value', () => {
  it('RTO replays the site failure that takes the longest to recover from', () => {
    const { report, o } = obj('report-three-az', 'rto')
    const a = objectiveAction(o, report, { rpsPeak: 3000, ruleCheck })
    expect(a?.kind).toBe('scenario')
    const worst = siteScenarios(report).filter((s) => s.recoverySeconds !== null).sort((x, y) => y.recoverySeconds! - x.recoverySeconds!)[0]
    expect(a).toEqual({ kind: 'scenario', scenario: worst.scenario })
    expect(worst.recoverySeconds).toBe(o.actual) // the number on the pill is the number of that scenario
  })

  it('RPO replays the site failure that loses the most', () => {
    const { report, o } = obj('report-three-az', 'rpo')
    const a = objectiveAction(o, report, { rpsPeak: 3000, ruleCheck })
    expect(a?.kind).toBe('scenario')
  })

  it('an objective without a requirement sends the user to the field to fill in', () => {
    const rto = obj('report-no-requirements', 'rto')
    expect(rto.o.status).toBe('no_data')
    expect(objectiveAction(rto.o, rto.report, { ruleCheck })).toEqual({ kind: 'fill', need: { target: 'requirements', prop: 'rtoSeconds' } })
    const rpo = obj('report-no-requirements', 'rpo')
    expect(objectiveAction(rpo.o, rpo.report, { ruleCheck })).toEqual({ kind: 'fill', need: { target: 'requirements', prop: 'rpoSeconds' } })
    const cap = obj('report-no-requirements', 'capacity')
    expect(objectiveAction(cap.o, cap.report, { rpsPeak: undefined, ruleCheck })).toEqual({ kind: 'fill', need: { target: 'requirements', prop: 'rpsPeak' } })
  })

  it('capacity opens the capacity plan once the peak load is known', () => {
    const { report, o } = obj('report-overloaded', 'capacity')
    expect(objectiveAction(o, report, { rpsPeak: 30000, ruleCheck })).toEqual({ kind: 'tab', tab: 'capacity' })
  })

  it('SPOF highlights exactly the single points of failure', () => {
    const { report, o } = obj('report-spof', 'spof')
    expect(o.actual).toBe(2)
    const a = objectiveAction(o, report, { rpsPeak: 5000, ruleCheck })
    expect(a?.kind).toBe('subjects')
    if (a?.kind === 'subjects') expect([...a.subjects.nodes].sort()).toEqual(['gslb', 'waf'])
    // nothing to show when the design has none
    const clean = obj('report-three-az', 'spof')
    expect(objectiveAction(clean.o, clean.report, { rpsPeak: 3000, ruleCheck })).toBeNull()
  })

  it('site loss points at a scenario of the failure domain, the one that hurts most', () => {
    const { report, o } = obj('report-three-az', 'site_loss')
    const a = objectiveAction(o, report, { rpsPeak: 3000, ruleCheck })
    expect(a?.kind).toBe('scenario')
    if (a?.kind === 'scenario') {
      const sites = siteScenarios(report)
      expect(sites.map((s) => s.scenario)).toContainEqual(a.scenario)
    }
  })
})
