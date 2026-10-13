// Where the traffic goes, as the engine computed it (`SimResult.traffic`, `trafficBefore`, `SimFrame.traffic`): pure helpers that turn the
// numbers into what the diagram draws. Every figure is relative to the load that enters at an entry point (1 = all of it); the engine
// guarantees that the links add up to the unit loads of the capacity table (see backend/crates/engine/tests/traffic.rs).

import type { Architecture, Detour, SimResult, Traffic } from '@/api/types'

/** Shares below this are not drawn: they are rounding. */
export const FLOW_EPS = 1e-6

/** The strongest flow of a picture. */
export function peak(t: Traffic | null | undefined): number {
  if (!t) return 0
  let m = 0
  for (const v of Object.values(t.flows)) m = Math.max(m, v)
  for (const d of t.detours ?? []) m = Math.max(m, d.flow)
  return m
}

/**
 * The flow that widths and densities are measured against: the same for the picture before the failure and the ones after it, so that a link
 * that has to carry twice as much *looks* like it carries twice as much. The entry link carries 1, which is the floor.
 */
export function reference(...pictures: (Traffic | null | undefined)[]): number {
  return Math.max(1, ...pictures.map(peak))
}

/** How wide a link is drawn for the flow it carries (px): a thin line for almost nothing, a trunk for everything. */
export function flowWidth(flow: number, ref: number, base = 1.3, extra = 3.7): number {
  if (flow <= FLOW_EPS || ref <= 0) return base
  return base + extra * Math.sqrt(Math.min(1, flow / ref))
}

/**
 * How many particles run along a link of `length` px for the flow it carries: they are `spacing` px apart on a link that carries the
 * reference flow and sparser on a weaker one. A link that carries anything has at least one; `scale` (0..1) thins them out for a big diagram.
 */
export function particleCount(flow: number, ref: number, length: number, spacing: number, scale = 1): number {
  if (flow <= FLOW_EPS || ref <= 0 || length <= 0 || spacing <= 0) return 0
  const n = (length / spacing) * Math.pow(Math.min(1, flow / ref), 0.6) * scale
  return Math.max(1, Math.min(MAX_PER_LINK, Math.round(n)))
}
/** However long and busy, one link does not get more than this many particles. */
export const MAX_PER_LINK = 12

/** The part of a link at which the requests that are lost on it are dropped: in the middle of a cut link, at the card for the rest. */
export const dropAt = (cut: boolean) => (cut ? 0.5 : 0.93)

/** What arrives at each node: the links into it and the detours into it. */
export function inflow(t: Traffic | null | undefined, arch: Pick<Architecture, 'edges'>): Record<string, number> {
  const out: Record<string, number> = {}
  if (!t) return out
  for (const e of arch.edges) {
    const f = t.flows[e.id]
    if (f) out[e.target] = (out[e.target] ?? 0) + f
  }
  for (const d of t.detours ?? []) out[d.to] = (out[d.to] ?? 0) + d.flow
  return out
}

export interface LoadChange {
  before: number
  now: number
  /** now / before; `Infinity` for a node that had no traffic and has some. */
  ratio: number
}

/** The nodes whose load is not what it was in normal operation by more than `tolerance` (a share of what it was). */
export function loadChanges(before: Traffic | null | undefined, now: Traffic | null | undefined, arch: Pick<Architecture, 'edges'>, tolerance = 0.12): Record<string, LoadChange> {
  const b = inflow(before, arch)
  const n = inflow(now, arch)
  const out: Record<string, LoadChange> = {}
  for (const id of new Set([...Object.keys(b), ...Object.keys(n)])) {
    const was = b[id] ?? 0
    const is = n[id] ?? 0
    if (was <= FLOW_EPS && is <= FLOW_EPS) continue
    const ratio = was <= FLOW_EPS ? Infinity : is / was
    if (Math.abs(ratio - 1) < tolerance) continue
    out[id] = { before: was, now: is, ratio }
  }
  return out
}

/** How a link's flow differs from normal operation: for the label on it. `null` while it carries what it always did. */
export function flowChange(before: number, now: number, tolerance = 0.12): 'up' | 'down' | 'off' | 'new' | null {
  if (before <= FLOW_EPS && now <= FLOW_EPS) return null
  if (before <= FLOW_EPS) return 'new'
  if (now <= FLOW_EPS) return 'off'
  const r = now / before
  if (Math.abs(r - 1) < tolerance) return null
  return r > 1 ? 'up' : 'down'
}

export interface Loss { edge: string; amount: number }

const biggestFirst = (m: Record<string, number> | undefined): Loss[] =>
  Object.entries(m ?? {})
    .filter(([, v]) => v > FLOW_EPS)
    .map(([edge, amount]) => ({ edge, amount }))
    .sort((a, b) => b.amount - a.amount || a.edge.localeCompare(b.edge))

/** Where requests are dropped so that they fail, the biggest loss first. */
export const lossesOf = (t: Traffic | null | undefined): Loss[] => biggestFirst(t?.lost)

/** Where calls go unanswered but the requests go on (a dependency the service can do without), the biggest first. */
export const missesOf = (t: Traffic | null | undefined): Loss[] => biggestFirst(t?.missed)

/** Detours that are in the picture, the biggest first. */
export function detoursOf(t: Traffic | null | undefined): Detour[] {
  return [...(t?.detours ?? [])].filter((d) => d.flow > FLOW_EPS || (d.lost ?? 0) > FLOW_EPS).sort((a, b) => b.flow - a.flow || a.from.localeCompare(b.from) || a.to.localeCompare(b.to))
}

/** The key of a detour between two nodes (the id of its edge on the canvas). */
export const detourKey = (from: string, to: string) => `${from}>${to}`

/** Every detour the story passes through, once: the canvas has an edge for each, drawn only in the pictures that have it. */
export function detourPairs(res: Pick<SimResult, 'traffic' | 'trafficBefore' | 'timeline'> | null | undefined): { from: string; to: string }[] {
  const seen = new Map<string, { from: string; to: string }>()
  const take = (t: Traffic | null | undefined) => {
    for (const d of t?.detours ?? []) seen.set(detourKey(d.from, d.to), { from: d.from, to: d.to })
  }
  take(res?.trafficBefore)
  for (const f of res?.timeline ?? []) take(f.traffic)
  take(res?.traffic)
  return [...seen.values()].sort((a, b) => detourKey(a.from, a.to).localeCompare(detourKey(b.from, b.to)))
}

/** Total of what is lost on links so that requests fail (relative to the entry load). */
export function lostTotal(t: Traffic | null | undefined): number {
  return Object.values(t?.lost ?? {}).reduce((s, v) => s + v, 0)
}

/** The relative figure as the user reads it: a percentage of the load that enters, or requests per second when the peak is declared. */
export function amountText(flow: number, rpsPeak: number | null | undefined, fmt: (n: number) => string): { value: string; unit: 'rps' | '%' } {
  if (rpsPeak && rpsPeak > 0) return { value: fmt(Math.round(flow * rpsPeak)), unit: 'rps' }
  return { value: fmt(Math.round(flow * 100)), unit: '%' }
}
