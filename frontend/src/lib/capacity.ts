import type { CapacityRow } from '@/api/types'

/** `unknown`: no capacity model; `over`: cannot carry the load; `tight`: carries it, but without the configured headroom. */
export type CapacityState = 'unknown' | 'ok' | 'tight' | 'over'

export interface CapacityWorstView {
  /** capacity left / load to carry in the worst failure */
  ratio: number
  load: number
  capacity: number
  /** the ratio reaches the configured minimum */
  ok: boolean
}

export interface CapacityView {
  row: CapacityRow
  load: number
  /** load / capacity; `null` without a capacity model */
  utilization: number | null
  state: CapacityState
  /** capacity that keeps the configured headroom at this load */
  target: number
  needed: number | null
  /** instances to add to reach `needed` (0 when enough) */
  add: number | null
  worst: CapacityWorstView | null
}

/**
 * The capacity plan of a service when the peak load is multiplied by `factor`. The engine reports the plan for
 * the declared load; load and the capacity it needs scale linearly, so "what if traffic doubles" needs no new
 * evaluation (at `factor` 1 the engine's own numbers are shown).
 */
export function capacityView(row: CapacityRow, factor: number, minDuringFailure: number): CapacityView {
  const load = row.loadRps * factor
  const target = row.targetRps * factor
  const cap = row.capacityRps
  const utilization = cap !== null && cap > 0 ? load / cap : null
  const state: CapacityState = cap === null || cap <= 0 ? 'unknown' : cap < load ? 'over' : cap < target ? 'tight' : 'ok'
  const needed = factor === 1 ? row.instancesNeeded : row.perReplicaRps && row.perReplicaRps > 0 ? Math.max(1, Math.ceil(target / row.perReplicaRps - 1e-9)) : null
  const w = row.worst
  const worst = w && w.loadRps > 0 ? { ratio: w.capacityRps / (w.loadRps * factor), load: w.loadRps * factor, capacity: w.capacityRps, ok: w.capacityRps / (w.loadRps * factor) >= minDuringFailure } : null
  return { row, load, utilization, state, target, needed, add: needed === null ? null : Math.max(0, needed - row.instances), worst }
}

export interface Bottleneck { factor: number; row: CapacityRow }

/** By how much the load can grow before the first service is overloaded (and which one gives out first). */
export function loadHeadroom(rows: CapacityRow[]): Bottleneck | null {
  let best: Bottleneck | null = null
  for (const r of rows) {
    if (r.capacityRps === null || r.capacityRps <= 0 || r.loadRps <= 0) continue
    const f = r.capacityRps / r.loadRps
    if (!best || f < best.factor) best = { factor: f, row: r }
  }
  return best
}

/** By how much the load can grow while every service still keeps `minDuringFailure` of the needed capacity in its worst failure. */
export function failureHeadroom(rows: CapacityRow[], minDuringFailure: number): Bottleneck | null {
  let best: Bottleneck | null = null
  const min = Math.max(minDuringFailure, 0.01)
  for (const r of rows) {
    const w = r.worst
    if (!w || w.loadRps <= 0) continue
    const f = w.capacityRps / (w.loadRps * min)
    if (!best || f < best.factor) best = { factor: f, row: r }
  }
  return best
}

export function stateCounts(views: CapacityView[]): Record<CapacityState, number> {
  const out: Record<CapacityState, number> = { unknown: 0, ok: 0, tight: 0, over: 0 }
  for (const v of views) out[v.state]++
  return out
}
