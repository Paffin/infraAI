import type { Architecture, Scenario, SimEvent, SimResult } from '@/api/types'
import { t } from '@/i18n'
import type { CatalogIndex } from './catalog'
import { fmtDuration, fmtPct, round } from './format'

const nameOf = (arch: Architecture, kind: 'nodes' | 'zones', id: string) => (arch[kind] as { id: string; name: string }[]).find((x) => x.id === id)?.name ?? id

/** Human label of a failure scenario. `units` maps unit keys to display names (from the report). */
export function scenarioLabel(sc: Scenario, arch: Architecture, units: Record<string, string> = {}): string {
  switch (sc.type) {
    case 'none': return t('sim.sc.none')
    case 'instance': return t('sim.sc.instance', { name: nameOf(arch, 'nodes', sc.nodeId), n: sc.count ?? 1 })
    case 'node': return t('sim.sc.node', { name: nameOf(arch, 'nodes', sc.nodeId) })
    case 'unit': return t('sim.sc.unit', { name: units[sc.unit] ?? sc.unit })
    case 'zone': return nameOf(arch, 'zones', sc.zoneId)
    case 'link': return t('sim.sc.link', { a: shortZone(nameOf(arch, 'zones', sc.a)), b: shortZone(nameOf(arch, 'zones', sc.b)) })
    case 'isolate': return t('sim.sc.isolate', { names: sc.zoneIds.map((z) => shortZone(nameOf(arch, 'zones', z))).join(' + ') })
    case 'sites': return sc.zoneIds.map((z) => shortZone(nameOf(arch, 'zones', z))).join(' + ')
    case 'geo': return t('sim.sc.geo', { name: sc.geo })
  }
}

/** `ЦОД-1 · Москва` → `ЦОД-1`: buttons need the short form. */
export const shortZone = (name: string) => name.split(' · ')[0]

/** Which family a scenario belongs to – the filter chips of the failure matrix. */
export type ScenarioFamily = 'zone' | 'rack' | 'isolate' | 'link' | 'sites' | 'geo' | 'other'
export function scenarioFamily(sc: Scenario, arch: Architecture, ix: CatalogIndex): ScenarioFamily {
  switch (sc.type) {
    case 'zone': {
      const z = arch.zones.find((x) => x.id === sc.zoneId)
      return z && ix.zoneTypes.get(z.type)?.level === 'rack' ? 'rack' : 'zone'
    }
    case 'isolate': return 'isolate'
    case 'link': return 'link'
    case 'sites': return 'sites'
    case 'geo': return 'geo'
    default: return 'other'
  }
}

export function sameScenario(a: Scenario | null | undefined, b: Scenario | null | undefined): boolean {
  return !!a && !!b && JSON.stringify(a) === JSON.stringify(b)
}

function rounded(v: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = {}
  for (const [k, x] of Object.entries(v)) out[k] = typeof x === 'number' ? round(x, 2) : x
  return out
}

export function eventText(e: SimEvent): string {
  const v = rounded(e.vars ?? {})
  switch (e.code) {
    case 'promoted': return v.rpo == null ? t('event.promoted.norpo', v) : t('event.promoted', v)
    case 'manual_failover': return v.noQuorum ? t('event.manual_failover.noquorum', v) : t('event.manual_failover', v)
    case 'scale_out': return v.to == null ? t('event.scale_out.generic', v) : t('event.scale_out', v)
    case 'restore_from_backup':
    case 'replicated_elsewhere':
      return t('event.' + e.code, { ...v, rpo: typeof v.rpo === 'number' ? v.rpo : 0 })
    case 'capacity_overload': return t('event.capacity_overload', { ...v, pct: typeof v.ratio === 'number' ? Math.round(v.ratio * 100) : '?' })
    default: return t('event.' + e.code, v)
  }
}

export interface ResultChip { text: string; tone: 'ok' | 'risk' | 'crit' | 'info' }

/** The one-line outcome shown in the what-if bar and the failure matrix. */
export function resultChips(r: Pick<SimResult, 'state' | 'recovery' | 'rpoSeconds' | 'rpoAssumed' | 'capacityRatio'> & { splitBrain: unknown; dataLost: unknown }): ResultChip[] {
  const out: ResultChip[] = []
  const rec = r.recovery
  if (rec.kind === 'auto') out.push({ text: t('sim.result.switch', { s: fmtDuration(rec.seconds) }), tone: 'info' })
  else if (rec.kind === 'manual') out.push({ text: t('sim.result.manual', { s: fmtDuration(rec.seconds) }), tone: 'risk' })
  else if (rec.kind === 'never') out.push({ text: t('sim.result.never'), tone: 'crit' })
  if (r.rpoSeconds === null) out.push({ text: t('sim.result.rpo.unknown'), tone: 'info' })
  else out.push({ text: t('sim.result.rpo', { v: r.rpoSeconds === 0 ? '0' : fmtDuration(r.rpoSeconds) + (r.rpoAssumed ? ' ~' : '') }), tone: r.rpoSeconds === 0 ? 'ok' : 'risk' })
  if (r.capacityRatio !== null && r.capacityRatio < 1) out.push({ text: t('sim.result.capacity', { pct: fmtPct(r.capacityRatio).replace('%', '') }), tone: r.capacityRatio < 0.7 ? 'risk' : 'info' })
  const sb = Array.isArray(r.splitBrain) ? r.splitBrain.length > 0 : !!r.splitBrain
  const dl = Array.isArray(r.dataLost) ? r.dataLost.length > 0 : !!r.dataLost
  if (sb) out.push({ text: t('sim.result.splitbrain'), tone: 'crit' })
  if (dl) out.push({ text: t('sim.result.dataLost'), tone: 'crit' })
  return out
}

export const stateKey = (s: string) => 'sim.result.' + s
