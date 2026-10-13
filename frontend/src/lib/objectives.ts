import type { Need, Objective, Report, Scenario, ScenarioSummary } from '@/api/types'

/** What a click on an objective pill does: the pill is a doorway to the evidence behind its value. */
export type ObjectiveAction =
  | { kind: 'scenario'; scenario: Scenario }
  | { kind: 'fill'; need: Need }
  | { kind: 'tab'; tab: 'capacity' }
  | { kind: 'subjects'; subjects: { nodes: string[]; edges: string[]; zones: string[] } }

export interface ObjectiveContext {
  rpsPeak?: number
  /** rule id → check type (`spof`, `rpo`…), from the rule catalog */
  ruleCheck: (ruleId: string) => string | undefined
}

const STATE_RANK = { down: 0, degraded: 1, up: 2 } as const

/** The failure scenarios the objectives are measured on: one per site. */
export function siteScenarios(report: Pick<Report, 'scenarios'>): ScenarioSummary[] {
  return report.scenarios.filter((s) => s.scenario.type === 'zone')
}

const maxBy = <T>(items: T[], key: (x: T) => number): T | undefined => items.reduce<T | undefined>((best, x) => (best === undefined || key(x) > key(best) ? x : best), undefined)

/** Worst scenario for the objective, in the same order of importance the engine uses to judge it. */
function worstScenario(id: string, sites: ScenarioSummary[]): ScenarioSummary | undefined {
  if (id === 'rto') {
    return sites.find((s) => s.recoveryKind === 'never') ?? maxBy(sites.filter((s) => s.recoverySeconds !== null), (s) => s.recoverySeconds!)
  }
  if (id === 'rpo') {
    return sites.find((s) => s.dataLost) ?? sites.find((s) => s.rpoSeconds === null) ?? maxBy(sites, (s) => s.rpoSeconds ?? 0)
  }
  if (id === 'site_loss') {
    const dead = sites.find((s) => s.state === 'down' && s.recoveryKind === 'never')
    return dead ?? [...sites].sort((a, b) => STATE_RANK[a.state] - STATE_RANK[b.state] || (a.capacityRatio ?? 1) - (b.capacityRatio ?? 1))[0]
  }
  return undefined
}

export function objectiveAction(o: Objective, report: Report, ctx: ObjectiveContext): ObjectiveAction | null {
  const sites = siteScenarios(report)
  switch (o.id) {
    case 'rto':
    case 'rpo': {
      if (o.required === null || o.required === undefined) return { kind: 'fill', need: { target: 'requirements', prop: o.id === 'rto' ? 'rtoSeconds' : 'rpoSeconds' } }
      const sc = worstScenario(o.id, sites) ?? sites[0]
      return sc ? { kind: 'scenario', scenario: sc.scenario } : null
    }
    case 'site_loss': {
      const sc = worstScenario(o.id, sites)
      return sc ? { kind: 'scenario', scenario: sc.scenario } : null
    }
    case 'spof': {
      const spofs = report.findings.filter((f) => !f.accepted && ctx.ruleCheck(f.ruleId) === 'spof')
      if (!spofs.length) return null
      const subjects = { nodes: [...new Set(spofs.flatMap((f) => f.subjects.nodes))], edges: [...new Set(spofs.flatMap((f) => f.subjects.edges))], zones: [...new Set(spofs.flatMap((f) => f.subjects.zones))] }
      return { kind: 'subjects', subjects }
    }
    case 'capacity':
      if (ctx.rpsPeak === undefined || ctx.rpsPeak === null) return { kind: 'fill', need: { target: 'requirements', prop: 'rpsPeak' } }
      return { kind: 'tab', tab: 'capacity' }
    default:
      return null
  }
}
