import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { beforeAll, describe, expect, it } from 'vitest'
import type { Architecture, Scenario, SimResult } from '../src/api/types'
import { setLang } from '../src/i18n'
import { stepsOf } from '../src/lib/playback'
import { scenarioLabel } from '../src/lib/sim'
import { impactOf, TIER } from '../src/lib/simImpact'
import { decodeScenario, encodeScenario, isScenario, linkTo, MAX_LINK_CHARS, scenarioFits, SIM_PARAM } from '../src/lib/simLink'
import { storyMarkdown } from '../src/lib/simStory'
import { tilesOf } from '../src/lib/simTiles'

/** Real engine output (`deploy/record-sim.sh`), see tests/fixtures/README.md. */
function recorded(name: string): { architecture: Architecture; scenario: Scenario; result: SimResult } {
  return JSON.parse(readFileSync(join(process.cwd(), 'tests', 'fixtures', `sim-${name}.json`), 'utf8'))
}

beforeAll(() => setLang('ru'))

describe('who a failure hits', () => {
  const { architecture: arch, result } = recorded('two-dc-zone')

  it('puts every node that is not as usual in exactly one group, worst first', () => {
    const impact = impactOf(result.nodeStates, arch.nodes, arch.zones)
    const notUp = arch.nodes.filter((n) => result.nodeStates[n.id] !== 'up')
    expect(notUp.length).toBeGreaterThan(0)
    const listed = [...impact.out, ...impact.hurt, ...impact.took].map((h) => h.id)
    expect(listed.sort()).toEqual(notUp.map((n) => n.id).sort())
    expect(impact.nodes).toBe(arch.nodes.length)
    expect(impact.affected).toBe(impact.out.length + impact.hurt.length)
    for (const h of impact.out) expect(TIER[h.state]).toBe('out')
    for (const h of impact.hurt) expect(TIER[h.state]).toBe('hurt')
    for (const h of impact.took) expect(TIER[h.state]).toBe('took')
    // what is lost comes before what only waits
    const rank = (s: string) => ['down', 'fenced', 'isolated', 'stalled'].indexOf(s)
    const ranks = impact.out.map((h) => rank(h.state))
    expect(ranks).toEqual([...ranks].sort((a, b) => a - b))
  })

  it('names the node, the zone it sits in, and the one that took over', () => {
    const impact = impactOf(result.nodeStates, arch.nodes, arch.zones)
    const primary = impact.out.find((h) => h.id === 'pg-primary')!
    expect(primary.name).toBe(arch.nodes.find((n) => n.id === 'pg-primary')!.name)
    expect(primary.zone).toBe(arch.zones.find((z) => z.id === arch.nodes.find((n) => n.id === 'pg-primary')!.zoneId)!.name)
    expect(impact.took.map((h) => h.id)).toContain('pg-replica')
  })

  it('has nobody in normal operation', () => {
    const calm = Object.fromEntries(arch.nodes.map((n) => [n.id, 'up' as const]))
    const impact = impactOf(calm, arch.nodes, arch.zones)
    expect(impact).toMatchObject({ out: [], hurt: [], took: [], affected: 0 })
  })

  it('tells a node that serves with losses from one that does not serve', () => {
    const states = { ...Object.fromEntries(arch.nodes.map((n) => [n.id, 'up' as const])), 'app-dc1': 'partial' as const, 'app-dc2': 'stalled' as const }
    const impact = impactOf(states, arch.nodes, arch.zones)
    expect(impact.hurt.map((h) => h.id)).toEqual(['app-dc1'])
    expect(impact.out.map((h) => h.id)).toEqual(['app-dc2'])
  })
})

describe('a failure as a link', () => {
  const { architecture: arch } = recorded('two-dc-zone')
  const all: Scenario[] = [
    { type: 'none' },
    { type: 'instance', nodeId: 'app-dc1', count: 2 },
    { type: 'node', nodeId: 'app-dc1' },
    { type: 'unit', unit: 'pg' },
    { type: 'zone', zoneId: 'dc1' },
    { type: 'link', a: 'dc1', b: 'dc2' },
    { type: 'isolate', zoneIds: ['dc1', 'site3'] },
    { type: 'sites', zoneIds: ['dc1', 'dc2'] },
    { type: 'geo', geo: 'Москва' },
    { type: 'combo', items: [{ type: 'zone', zoneId: 'dc1' }, { type: 'node', nodeId: 'pg-replica' }, { type: 'link', a: 'dc2', b: 'site3' }] },
  ]

  it('goes into the address and comes back as it was, in any language of the ids', () => {
    for (const sc of all) expect(decodeScenario(encodeScenario(sc)), JSON.stringify(sc)).toEqual(sc)
    const cyr: Scenario = { type: 'combo', items: [{ type: 'zone', zoneId: 'площадка-1' }, { type: 'node', nodeId: 'база «основная»' }] }
    expect(decodeScenario(encodeScenario(cyr))).toEqual(cyr)
    expect(encodeScenario(cyr)).toMatch(/^[A-Za-z0-9_-]+$/)
  })

  it('refuses what is not a scenario', () => {
    const b64 = (v: unknown) => encodeScenario(v as Scenario)
    for (const bad of [null, undefined, '', 'not base64 at all!', '%%%', encodeScenario({ type: 'node' } as Scenario), b64({ type: 'dance', nodeId: 'a' }), b64([1, 2]), b64('zone'), b64({ type: 'sites', zoneIds: [] }), b64({ type: 'combo', items: [] }), b64({ type: 'instance', nodeId: 'a', count: 0 }), b64({ type: 'instance', nodeId: 'a', count: 1.5 })]) {
      expect(decodeScenario(bad as string), String(bad)).toBeNull()
    }
    expect(isScenario({ type: 'zone', zoneId: 'x'.repeat(500) })).toBe(false)
    // a combination inside a combination inside… stops at a depth no one needs
    let deep: unknown = { type: 'zone', zoneId: 'a' }
    for (let i = 0; i < 8; i++) deep = { type: 'combo', items: [deep] }
    expect(isScenario(deep)).toBe(false)
    expect(isScenario({ type: 'combo', items: Array.from({ length: 101 }, () => ({ type: 'zone', zoneId: 'a' })) })).toBe(false)
  })

  it('fits the diagram it names things of, and only that', () => {
    for (const sc of all.filter((s) => ['none', 'zone', 'link', 'sites', 'node', 'instance'].includes(s.type))) expect(scenarioFits(sc, arch), JSON.stringify(sc)).toBe(true)
    expect(scenarioFits({ type: 'zone', zoneId: 'dc9' }, arch)).toBe(false)
    expect(scenarioFits({ type: 'node', nodeId: 'ghost' }, arch)).toBe(false)
    expect(scenarioFits({ type: 'link', a: 'dc1', b: 'nowhere' }, arch)).toBe(false)
    expect(scenarioFits({ type: 'sites', zoneIds: ['dc1', 'dc9'] }, arch)).toBe(false)
    expect(scenarioFits({ type: 'combo', items: [{ type: 'zone', zoneId: 'dc1' }, { type: 'node', nodeId: 'ghost' }] }, arch)).toBe(false)
    expect(scenarioFits({ type: 'unit', unit: 'pg' }, arch, new Set(['pg']))).toBe(true)
    expect(scenarioFits({ type: 'unit', unit: 'zz' }, arch, new Set(['pg']))).toBe(false)
  })

  it('is the address of the page with the scenario in it, and nothing else', () => {
    const sc: Scenario = { type: 'zone', zoneId: 'dc1' }
    const link = linkTo(sc, 'https://arch.example.com/p/abc123?old=1#frag')!
    const url = new URL(link)
    expect(url.pathname).toBe('/p/abc123')
    expect([...url.searchParams.keys()]).toEqual([SIM_PARAM])
    expect(url.hash).toBe('')
    expect(decodeScenario(url.searchParams.get(SIM_PARAM))).toEqual(sc)
    // too big for an address: no link rather than one a browser cuts
    const huge: Scenario = { type: 'combo', items: Array.from({ length: 100 }, (_, i) => ({ type: 'node' as const, nodeId: `a-rather-long-node-identifier-${i}` })) }
    expect(isScenario(huge)).toBe(true)
    expect(linkTo(huge, 'https://arch.example.com/p/abc123')).toBeNull()
    expect(MAX_LINK_CHARS).toBeGreaterThan(500)
  })

  it('is read back into a label that names every failure', () => {
    const sc: Scenario = { type: 'combo', items: [{ type: 'zone', zoneId: 'dc1' }, { type: 'node', nodeId: 'pg-replica' }] }
    const label = scenarioLabel(sc, arch)
    expect(label).toContain('ЦОД-1')
    expect(label).not.toContain('Москва')
    expect(label).toContain('PostgreSQL replica')
    expect(label).toContain(' + ')
  })
})

describe('the four figures of a failure', () => {
  const calm = (arch: Architecture, states: SimResult['nodeStates']) => impactOf(states, arch.nodes, arch.zones)

  it('say an automatic switch of a replica in seconds, against the goal', () => {
    const { architecture: arch, result } = recorded('two-dc-zone')
    const impact = calm(arch, result.nodeStates)
    const ctx = { requirements: { rtoSeconds: 900, rpoSeconds: 0 }, units: {}, capacityRatio: result.capacityRatio, impact }
    const [rto, rpo, capacity, affected] = tilesOf(result, ctx)
    expect([rto.id, rpo.id, capacity.id, affected.id]).toEqual(['rto', 'rpo', 'capacity', 'affected'])
    expect(result.recovery.kind).toBe('auto')
    expect(rto.value).toContain('35')
    expect(rto.tone).toBe('ok')
    expect(rto.hint).toContain('15')
    // the goal is stricter than the switch: the same recovery is a miss
    expect(tilesOf(result, { ...ctx, requirements: { rtoSeconds: 10 } })[0].tone).toBe('crit')
    expect(affected.value).toMatch(/\d+.*\d+/)
    expect(affected.tone).toBe('risk')
  })

  it('say that a person has to do it, and that data is gone, when that is so', () => {
    const { architecture: arch, result } = recorded('two-dc-sites')
    expect(result.recovery.kind).toBe('manual')
    expect(result.dataLost.length).toBeGreaterThan(0)
    const ctx = { requirements: {}, units: { 'kafka-dc1': 'Kafka DC1' }, capacityRatio: result.capacityRatio, impact: calm(arch, result.nodeStates) }
    const [rto, rpo] = tilesOf(result, ctx)
    expect(rto.tone).toBe('risk')
    expect(rto.hint).toContain('вручную')
    expect(rpo.tone).toBe('crit')
    expect(rpo.hint).toContain('Kafka DC1')
    // a unit without a display name is named by its key, not left out
    expect(rpo.hint).toContain('kafka-dc2')
  })

  it('measure capacity against the peak, and say nothing before the failure', () => {
    const { architecture: arch, result } = recorded('two-dc-zone')
    const impact = calm(arch, result.nodeStates)
    const base = { requirements: {}, units: {}, impact }
    expect(tilesOf(result, { ...base, capacityRatio: 1.4 })[2]).toMatchObject({ value: '140%', tone: 'ok' })
    expect(tilesOf(result, { ...base, capacityRatio: 0.8 })[2].tone).toBe('risk')
    expect(tilesOf(result, { ...base, capacityRatio: 0.4 })[2].tone).toBe('crit')
    const before = tilesOf(result, { ...base, capacityRatio: null, normal: true })[2]
    expect(before).toMatchObject({ value: '—', hint: '', tone: '' })
    expect(tilesOf(result, { ...base, capacityRatio: null })[2].hint).not.toBe('')
  })
})

describe('the written account of a failure', () => {
  const { architecture: arch, result } = recorded('two-dc-combo')
  const steps = stepsOf(result)
  const impact = impactOf(result.nodeStates, arch.nodes, arch.zones)
  const tiles = tilesOf(result, { requirements: { rtoSeconds: 900 }, units: {}, capacityRatio: result.capacityRatio, impact })
  const input = {
    title: 'Симуляция: ЦОД-1 + PostgreSQL replica', res: result, state: result.state, steps, tiles, impact,
    served: result.traffic!.served, servedBefore: result.trafficBefore!.served, rps: 5000, date: '2026-10-13',
  }

  it('is Markdown with the outcome, the traffic, every step and who was hit', () => {
    const md = storyMarkdown(input)
    expect(md.startsWith('# Симуляция: ЦОД-1 + PostgreSQL replica\n')).toBe(true)
    expect(md).toContain('**Итог:** сервис недоступен.')
    for (const tile of tiles) expect(md).toContain(`${tile.label}: **${tile.value}**`)
    expect(md).toMatch(/\*\*Трафик:\*\* 0% запросов обслуживается \(в норме — \d+%\)/)
    // a heading per step, in the order of the story, the first is normal operation
    const headings = md.split('\n').filter((l) => l.startsWith('### '))
    expect(headings).toHaveLength(steps.length)
    expect(headings[0]).toContain('до отказа')
    expect(md).toContain('## Кого затронуло')
    for (const h of impact.out) expect(md).toContain(`«${h.name}»`)
    expect(md.trimEnd().endsWith('2026-10-13_')).toBe(true)
  })

  it('says the same in English when the interface does', () => {
    setLang('en')
    try {
      const md = storyMarkdown({ ...input, title: 'Simulation: DC-1 + PostgreSQL replica', tiles: tilesOf(result, { requirements: {}, units: {}, capacityRatio: result.capacityRatio, impact }) })
      expect(md).toContain('**Outcome:**')
      expect(md).toContain('## Who is hit')
      expect(md).toContain('### before the failure')
      // what the account itself says is English; only the names of the design (zones, nodes) stay as they are
      for (const heading of md.split('\n').filter((l) => l.startsWith('#'))) expect(heading, heading).not.toMatch(/[А-Яа-я]/)
    } finally {
      setLang('ru')
    }
  })

  it('has the events as they are when the failure has no story', () => {
    const md = storyMarkdown({ ...input, steps: [] })
    expect(md).not.toContain('### ')
    expect(result.events.length).toBeGreaterThan(0)
    expect(md.split('\n').filter((l) => l.startsWith('- ')).length).toBeGreaterThanOrEqual(result.events.length)
  })
})
