import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import type { Architecture, Scenario, SimResult } from '../src/api/types'
import { amountText, detourKey, detourPairs, detoursOf, dropAt, flowChange, flowWidth, inflow, loadChanges, lossesOf, lostTotal, MAX_PER_LINK, particleCount, peak, reference } from '../src/lib/traffic'

/** Real engine output (`deploy/record-sim.sh`): the simulation of a failure with the traffic of every picture. */
function recorded(name: string): { architecture: Architecture; scenario: Scenario; result: SimResult } {
  return JSON.parse(readFileSync(join(process.cwd(), 'tests', 'fixtures', `sim-${name}.json`), 'utf8'))
}

describe('the traffic the engine reports', () => {
  const { architecture: arch, result } = recorded('two-dc-zone')
  const before = result.trafficBefore!
  const failure = result.timeline![0].traffic!

  it('is measured against the entry: the busiest link carries all of it, and widths are the same in every picture', () => {
    expect(peak(before)).toBe(1)
    expect(peak(null)).toBe(0)
    expect(reference(before, failure, result.traffic)).toBe(1)
    expect(reference(null, undefined)).toBe(1)
    // a link that has to carry twice what it carried is drawn wider than it was
    expect(flowWidth(1, 1)).toBeGreaterThan(flowWidth(0.5, 1))
    expect(flowWidth(0.5, 1)).toBeGreaterThan(flowWidth(0.05, 1))
    expect(flowWidth(0, 1)).toBe(1.3)
    expect(flowWidth(5, 1)).toBe(flowWidth(1, 1))
  })

  it('has the particles follow the flow: at least one for anything, more for more, never more than the cap', () => {
    expect(particleCount(0, 1, 300, 56)).toBe(0)
    expect(particleCount(0.01, 1, 300, 56)).toBe(1)
    expect(particleCount(1, 1, 300, 56)).toBeGreaterThan(particleCount(0.3, 1, 300, 56))
    expect(particleCount(1, 1, 600, 56)).toBeGreaterThan(particleCount(1, 1, 200, 56))
    expect(particleCount(1, 1, 1e6, 56)).toBe(MAX_PER_LINK)
    expect(particleCount(1, 1, 300, 28)).toBeGreaterThan(particleCount(1, 1, 300, 56))
    expect(particleCount(1, 1, 300, 56, 0.5)).toBeLessThan(particleCount(1, 1, 300, 56, 1))
    expect(particleCount(1, 1, 0, 56)).toBe(0)
    expect(particleCount(1, 1, 300, 0)).toBe(0)
  })

  it('adds up to what arrives at each card', () => {
    const b = inflow(before, arch)
    expect(b['waf']).toBe(1)
    expect(b['ingress-dc1']).toBeCloseTo(0.5, 6)
    expect(b['app-dc1']).toBeCloseTo(0.5, 6)
    expect(inflow(null, arch)).toEqual({})
    // the whole load now goes to the surviving site
    const f = inflow(failure, arch)
    expect(f['ingress-dc2']).toBeCloseTo(1, 6)
    expect(f['ingress-dc1']).toBeUndefined()
  })

  it('names the cards that carry more or less than in normal operation', () => {
    const changes = loadChanges(before, failure, arch)
    expect(changes['app-dc2'].ratio).toBeCloseTo(2, 5)
    expect(changes['app-dc2'].before).toBeCloseTo(0.5, 6)
    expect(changes['app-dc1'].now).toBe(0)
    expect(changes['waf']).toBeUndefined()
    // what changed by less than the tolerance is not a change
    expect(loadChanges(before, before, arch)).toEqual({})
    expect(loadChanges(before, failure, arch, 5)['app-dc2']).toBeUndefined()
  })

  it('says how a link differs from normal operation', () => {
    expect(flowChange(0.5, 0.5)).toBeNull()
    expect(flowChange(0.5, 0.52)).toBeNull()
    expect(flowChange(0.5, 1)).toBe('up')
    expect(flowChange(0.5, 0.2)).toBe('down')
    expect(flowChange(0.5, 0)).toBe('off')
    expect(flowChange(0, 0.5)).toBe('new')
    expect(flowChange(0, 0)).toBeNull()
  })

  it('lists where the requests are dropped, the biggest loss first, and what goes around', () => {
    const lost = lossesOf(failure)
    expect(lost.length).toBeGreaterThan(0)
    expect(lost.map((l) => l.amount)).toEqual([...lost.map((l) => l.amount)].sort((a, b) => b - a))
    expect(lost.some((l) => l.edge === 'e-app-dc2--pg-replica')).toBe(true)
    expect(lostTotal(failure)).toBeCloseTo(lost.reduce((s, l) => s + l.amount, 0), 9)
    expect(lossesOf(before)).toEqual([])
    expect(lossesOf(undefined)).toEqual([])
    expect(detoursOf(before)).toEqual([])
  })

  it('knows every detour of a story once', () => {
    const r = recorded('three-az-app').result
    const pairs = detourPairs(r)
    expect(pairs.length).toBeGreaterThan(0)
    expect(new Set(pairs.map((p) => detourKey(p.from, p.to))).size).toBe(pairs.length)
    expect(pairs.some((p) => p.from === 'lb-a' && p.to === 'app-b')).toBe(true)
    expect(detourPairs(null)).toEqual([])
    expect(detourPairs({ traffic: undefined, trafficBefore: undefined, timeline: undefined })).toEqual([])
  })

  it('writes an amount as a share of the load, or in requests per second when the peak is declared', () => {
    const fmt = (n: number) => String(n)
    expect(amountText(0.5, undefined, fmt)).toEqual({ value: '50', unit: '%' })
    expect(amountText(0.5, 0, fmt)).toEqual({ value: '50', unit: '%' })
    expect(amountText(0.5, 5000, fmt)).toEqual({ value: '2500', unit: 'rps' })
  })

  it('drops what is lost on a cut link in its middle and elsewhere at the card', () => {
    expect(dropAt(true)).toBe(0.5)
    expect(dropAt(false)).toBeGreaterThan(0.8)
  })
})

describe('on every recorded failure', () => {
  for (const name of ['two-dc-zone', 'three-az-app', 'three-az-link']) {
    it(`${name}: every picture is drawn within the measure and the particle cap`, () => {
      const { result } = recorded(name)
      const pictures = [result.trafficBefore!, ...result.timeline!.map((f) => f.traffic!), result.traffic!]
      const ref = reference(...pictures)
      for (const t of pictures) {
        expect(t.served).toBeGreaterThanOrEqual(0)
        expect(t.served).toBeLessThanOrEqual(1)
        for (const v of Object.values(t.flows)) {
          expect(v).toBeLessThanOrEqual(ref)
          expect(particleCount(v, ref, 400, 56)).toBeLessThanOrEqual(MAX_PER_LINK)
          expect(flowWidth(v, ref)).toBeLessThanOrEqual(5)
        }
      }
      // the story ends where the settled result is
      expect(result.timeline!.at(-1)!.traffic).toEqual(result.traffic)
    })
  }
})
