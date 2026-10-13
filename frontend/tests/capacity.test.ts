import { describe, expect, it } from 'vitest'
import { capacityView, failureHeadroom, loadHeadroom, stateCounts } from '../src/lib/capacity'
import { fixture } from './fixtures'

// the thresholds of the shipped engine settings (bootstrap: engine.minCapacityDuringFailure)
const MIN = 0.8

describe('capacity plan of the real engine report', () => {
  it('a healthy design carries its load with headroom and survives its worst failure', () => {
    const { report } = fixture('report-three-az')
    expect(report.capacity.length).toBeGreaterThan(3)
    const views = report.capacity.map((r) => capacityView(r, 1, MIN))
    for (const v of views.filter((x) => x.state !== 'unknown')) {
      expect(v.state, v.row.name).toBe('ok')
      expect(v.utilization).toBeGreaterThan(0)
      expect(v.utilization).toBeLessThan(1)
      expect(v.add, `${v.row.name}: no instances to add`).toBe(0)
    }
    // every service that has a capacity model also has a worst failure
    for (const v of views.filter((x) => x.row.capacityRps !== null)) expect(v.worst, v.row.name).not.toBeNull()
  })

  it('at ×1 the numbers are the engine\'s own', () => {
    const { report } = fixture('report-overloaded')
    for (const r of report.capacity) {
      const v = capacityView(r, 1, MIN)
      expect(v.load).toBe(r.loadRps)
      expect(v.target).toBe(r.targetRps)
      expect(v.needed).toBe(r.instancesNeeded)
    }
  })

  it('an overloaded design is flagged and says how many instances it lacks', () => {
    const { report } = fixture('report-overloaded')
    const views = report.capacity.map((r) => capacityView(r, 1, MIN))
    const counts = stateCounts(views)
    expect(counts.over + counts.tight).toBeGreaterThan(0)
    const bad = views.filter((v) => v.state === 'over' || v.state === 'tight')
    for (const v of bad) {
      expect(v.add, v.row.name).toBeGreaterThan(0)
      expect(v.needed!).toBeGreaterThan(v.row.instances)
    }
  })

  it('load and the capacity it needs scale linearly with the what-if factor', () => {
    const { report } = fixture('report-three-az')
    const row = report.capacity.find((r) => r.capacityRps !== null && r.perReplicaRps)!
    const one = capacityView(row, 1, MIN)
    const two = capacityView(row, 2, MIN)
    expect(two.load).toBeCloseTo(one.load * 2)
    expect(two.target).toBeCloseTo(one.target * 2)
    expect(two.utilization!).toBeCloseTo(one.utilization! * 2)
    expect(two.needed!).toBeGreaterThanOrEqual(one.needed!)
    expect(two.worst!.ratio).toBeCloseTo(one.worst!.ratio / 2)
  })

  it('the room before overload is where the first service flips to overloaded', () => {
    for (const name of ['report-three-az', 'report-overloaded']) {
      const { report } = fixture(name)
      const room = loadHeadroom(report.capacity)!
      expect(room, name).not.toBeNull()
      expect(capacityView(room.row, room.factor * 0.99, MIN).state).not.toBe('over')
      expect(capacityView(room.row, room.factor * 1.01, MIN).state).toBe('over')
      // no other service gives out earlier
      for (const r of report.capacity) if (r.capacityRps) expect(capacityView(r, room.factor * 0.99, MIN).state, `${name}/${r.name}`).not.toBe('over')
    }
  })

  it('the room during a failure is where the worst case drops below the configured minimum', () => {
    const { report } = fixture('report-three-az')
    const room = failureHeadroom(report.capacity, MIN)!
    expect(room).not.toBeNull()
    expect(capacityView(room.row, room.factor * 0.98, MIN).worst!.ok).toBe(true)
    expect(capacityView(room.row, room.factor * 1.02, MIN).worst!.ok).toBe(false)
    expect(capacityView(room.row, room.factor, MIN).worst!.ratio).toBeCloseTo(MIN, 6)
  })

  it('without a declared peak there is nothing to plan', () => {
    expect(fixture('report-no-requirements').report.capacity).toEqual([])
    expect(loadHeadroom([])).toBeNull()
    expect(failureHeadroom([], MIN)).toBeNull()
  })
})
