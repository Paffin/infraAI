import type { Architecture, Effect, Recommendation, Rule } from '@/api/types'
import { durationText, t } from '@/i18n'
import { fixKindLabel } from '@/lib/findings'
import { fmtMoney } from '@/lib/format'
import { formatAvailability } from '@/lib/nines'

/** What a recommendation does, in a few figures: good, bad (a trade the engine did not hide) or just a fact. */
export type ChipTone = 'good' | 'bad' | 'plain'
export interface EffectChip {
  id: 'critical' | 'risk' | 'score' | 'availability' | 'cost' | 'introduced'
  tone: ChipTone
  text: string
  /** the longer form for a tooltip */
  title?: string
}

/** The words for a recommended fix: the rule's own label for the kind of fix, with the variables of the finding and the fix. */
export function recommendationLabel(rule: Rule | undefined, r: Pick<Recommendation, 'kind' | 'vars'>): string {
  return fixKindLabel(rule, r.kind, r.vars)
}

/** Money a month with its sign: `+4 200 ₽`, `−1 000 ₽`. */
function signedMoney(delta: number, symbol: string): string {
  return (delta < 0 ? '−' : '+') + fmtMoney(Math.abs(delta), symbol)
}

/**
 * The figures of an effect that moved, worst-first honesty included: what the change costs and what it brings in
 * are both listed, and a finding it introduces is said as plainly as the ones it closes. Figures that did not change are not chips.
 */
export function effectChips(e: Effect, symbol: string): EffectChip[] {
  const out: EffectChip[] = []
  if (e.critical.before !== e.critical.after) {
    const rules = [...new Set(e.resolved.map((id) => id.split(':')[0]))]
    out.push({
      id: 'critical',
      tone: e.critical.after < e.critical.before ? 'good' : 'bad',
      text: t('rec.chip.critical', { from: e.critical.before, to: e.critical.after }),
      title: rules.length ? t('rec.chip.closes', { rules: rules.join(', ') }) : undefined,
    })
  }
  if (e.risk.before !== e.risk.after) {
    out.push({ id: 'risk', tone: e.risk.after < e.risk.before ? 'good' : 'bad', text: t('rec.chip.risk', { from: e.risk.before, to: e.risk.after }) })
  }
  const [s0, s1] = [Math.round(e.score.before), Math.round(e.score.after)]
  if (s0 !== s1) out.push({ id: 'score', tone: s1 > s0 ? 'good' : 'bad', text: t('rec.chip.score', { from: s0, to: s1 }) })
  if (e.availability) {
    const [a0, a1] = [formatAvailability(e.availability.before), formatAvailability(e.availability.after)]
    if (a0 !== a1) {
      out.push({
        id: 'availability',
        tone: e.availability.after > e.availability.before ? 'good' : 'bad',
        text: t('rec.chip.availability', { from: a0, to: a1 }),
        title: e.downtimeSeconds ? t('rec.chip.availability.title', { from: durationText(e.downtimeSeconds.before), to: durationText(e.downtimeSeconds.after) }) : undefined,
      })
    }
  }
  const dCost = e.cost.after - e.cost.before
  if (Math.abs(dCost) >= 0.5) out.push({ id: 'cost', tone: dCost < 0 ? 'good' : 'plain', text: t('rec.chip.cost', { v: signedMoney(dCost, symbol) }) })
  if (e.introduced.length) out.push({ id: 'introduced', tone: 'bad', text: t('rec.chip.introduced', { n: e.introduced.length }) })
  return out
}

/**
 * The design without where its cards lie: two architectures with the same fingerprint are the same design to the engine,
 * so what was computed for one holds for the other. Dragging a card does not change it; adding a replica does.
 */
export function archFingerprint(a: Architecture): string {
  const bare = <T extends { ui?: unknown }>(x: T): Omit<T, 'ui'> => {
    const { ui: _ui, ...rest } = x
    return rest
  }
  return JSON.stringify([a.requirements, a.zones.map(bare), a.nodes.map(bare), a.edges])
}
