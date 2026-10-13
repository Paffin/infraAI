// What a failure costs, in four figures an architect is asked about first: how long the service is interrupted, how much data is lost,
// how much of the peak load the rest can carry, and how many nodes are hit. The card of the failure shows them as tiles, the written
// account of it (lib/simStory.ts) says them in words; both come from here, so that they never disagree.

import type { Requirements, SimResult } from '@/api/types'
import { t } from '@/i18n'
import { fmtDuration, fmtPct } from './format'
import type { Impact } from './simImpact'

export type Tone = 'ok' | 'risk' | 'crit' | ''

export interface Tile {
  id: 'rto' | 'rpo' | 'capacity' | 'affected'
  label: string
  value: string
  hint: string
  tone: Tone
}

type Outcome = Pick<SimResult, 'recovery' | 'rpoSeconds' | 'rpoAssumed' | 'dataLost' | 'bottleneck'>

export interface TileContext {
  requirements: Pick<Requirements, 'rtoSeconds' | 'rpoSeconds'>
  /** display names of the units (clusters) by their keys */
  units: Record<string, string>
  /** the capacity left in the picture on screen: it changes while the recovery runs */
  capacityRatio: number | null
  impact: Impact
  /** the picture on screen is normal operation, before the failure: there is nothing to measure yet */
  normal?: boolean
}

const unitName = (units: Record<string, string>, key: string) => units[key] ?? key

function recovery(res: Outcome, req: TileContext['requirements']): Tile {
  const { kind, seconds } = res.recovery
  const goal = typeof req.rtoSeconds === 'number' ? t('simtile.goal', { v: fmtDuration(req.rtoSeconds) }) : ''
  const label = t('simtile.rto')
  if (kind === 'none') return { id: 'rto', label, value: t('simtile.rto.none'), hint: goal, tone: 'ok' }
  if (kind === 'never') return { id: 'rto', label, value: t('simtile.rto.never'), hint: t('simtile.rto.never.hint'), tone: 'crit' }
  const value = seconds === null ? '—' : fmtDuration(seconds)
  const how = t(kind === 'auto' ? 'simtile.rto.auto' : 'simtile.rto.manual')
  const hint = goal ? `${how} · ${goal}` : how
  // against the goal when there is one; without it an automatic switch is fine and a person's work is a risk
  const tone: Tone = typeof req.rtoSeconds === 'number' && seconds !== null ? (seconds <= req.rtoSeconds ? 'ok' : 'crit') : kind === 'auto' ? 'ok' : 'risk'
  return { id: 'rto', label, value, hint, tone }
}

function data(res: Outcome, ctx: TileContext): Tile {
  const label = t('simtile.rpo')
  if (res.dataLost.length) return { id: 'rpo', label, value: t('simtile.rpo.lost'), hint: res.dataLost.map((u) => unitName(ctx.units, u)).join(', '), tone: 'crit' }
  const goal = typeof ctx.requirements.rpoSeconds === 'number' ? t('simtile.goal', { v: ctx.requirements.rpoSeconds === 0 ? '0' : fmtDuration(ctx.requirements.rpoSeconds) }) : ''
  if (res.rpoSeconds === null) return { id: 'rpo', label, value: t('simtile.rpo.unknown'), hint: t('simtile.rpo.unknown.hint'), tone: '' }
  if (res.rpoSeconds === 0) return { id: 'rpo', label, value: t('simtile.rpo.none'), hint: goal || t('simtile.rpo.sync'), tone: 'ok' }
  const value = fmtDuration(res.rpoSeconds) + (res.rpoAssumed ? ' ~' : '')
  const tone: Tone = typeof ctx.requirements.rpoSeconds === 'number' ? (res.rpoSeconds <= ctx.requirements.rpoSeconds ? 'ok' : 'crit') : 'risk'
  return { id: 'rpo', label, value, hint: goal || (res.rpoAssumed ? t('sim.assumed') : ''), tone }
}

function capacity(res: Outcome, ctx: TileContext): Tile {
  const label = t('simtile.capacity')
  const r = ctx.capacityRatio
  if (r === null) return { id: 'capacity', label, value: '—', hint: ctx.normal ? '' : t('simtile.capacity.unknown'), tone: '' }
  const bottleneck = res.bottleneck ? t('simtile.capacity.bottleneck', { name: unitName(ctx.units, res.bottleneck) }) : ''
  return { id: 'capacity', label, value: fmtPct(r), hint: r >= 1 ? t('simtile.capacity.ok') : bottleneck || t('simtile.capacity.short'), tone: r >= 1 ? 'ok' : r >= 0.7 ? 'risk' : 'crit' }
}

function affected(ctx: TileContext): Tile {
  const { impact } = ctx
  const label = t('simtile.affected')
  const value = t('simtile.affected.value', { n: impact.affected, total: impact.nodes })
  const parts = [impact.out.length ? t('simtile.affected.out', { n: impact.out.length }) : '', impact.hurt.length ? t('simtile.affected.hurt', { n: impact.hurt.length }) : '', impact.took.length ? t('simtile.affected.took', { n: impact.took.length }) : ''].filter(Boolean)
  return { id: 'affected', label, value, hint: parts.join(' · ') || t('simtile.affected.none'), tone: impact.affected ? 'risk' : 'ok' }
}

/** The four tiles of a failure: `res` is its outcome, `ctx` what it is measured against and the picture on screen. */
export function tilesOf(res: Outcome, ctx: TileContext): Tile[] {
  return [recovery(res, ctx.requirements), data(res, ctx), capacity(res, ctx), affected(ctx)]
}
