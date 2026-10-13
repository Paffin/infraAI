import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Architecture, Scenario, SimResult } from '../src/api/types'
import { layoutAll } from '../src/editor/layout'
import { pictures, stepAtPercent, stepsOf, story, trackPosition } from '../src/lib/playback'
import { useApp } from '../src/stores/app'
import { useEditor } from '../src/stores/editor'
import { usePlayback } from '../src/stores/playback'
import { realIndex } from './fixtures'

/** Real engine output (`deploy/record-sim.sh`): the simulation of a failure with its timeline. */
function recorded(name: string): { architecture: Architecture; scenario: Scenario; result: SimResult } {
  return JSON.parse(readFileSync(join(process.cwd(), 'tests', 'fixtures', `sim-${name}.json`), 'utf8'))
}

describe('the frames the engine returns', () => {
  const { result } = recorded('two-dc-zone')

  it('add up to pictures that end in the settled result', () => {
    const p = pictures(result.timeline!)
    expect(p).toHaveLength(result.timeline!.length)
    expect(Object.keys(p[0].nodes)).toHaveLength(Object.keys(result.nodeStates).length)
    expect(p[p.length - 1].nodes).toEqual(result.nodeStates)
    expect(p[p.length - 1].edges).toEqual(result.edgeStates)
    expect(p[0].changed).toEqual([])
    for (let i = 1; i < p.length; i++) expect(p[i].changed.sort(), `step ${i}`).toEqual(Object.keys(result.timeline![i].nodes).sort())
  })

  it('tell the story in steps: normal operation first, then what happens with the failure, then each step in the frame of its moment', () => {
    const steps = stepsOf(result)
    expect(steps.map((s) => s.kind)).toEqual(['before', 'failure', ...result.timeline!.slice(1).map(() => 'step')])
    expect(steps.slice(1).map((s) => s.at)).toEqual(result.timeline!.map((f) => f.at))
    expect(steps[1].at).toBe(0)
    expect(steps[0].events, 'nothing has happened yet').toEqual([])
    const timed = result.events.filter((e) => (e.at ?? 0) > 0)
    expect(timed.length).toBeGreaterThan(0)
    for (const e of timed) expect(steps.find((s) => s.kind !== 'before' && s.at === e.at)!.events).toContain(e)
    for (const e of result.events.filter((x) => x.at === undefined)) expect(steps[1].events).toContain(e)
    // every event is in exactly one step
    expect(steps.flatMap((s) => s.events)).toHaveLength(result.events.length)
    // and each step says how much of the traffic is served
    expect(steps.map((s) => s.served)).toEqual([result.trafficBefore!.served, ...result.timeline!.map((f) => f.traffic!.served)])
  })

  it('show the promoted replica only from the moment of its promotion', () => {
    const p = pictures(result.timeline!)
    const promoted = result.events.find((e) => e.code === 'promoted')!
    const idx = result.timeline!.findIndex((f) => f.at === promoted.at)
    expect(p[idx].nodes[promoted.node!]).toBe('promoted')
    expect(p[idx - 1].nodes[promoted.node!]).not.toBe('promoted')
    // and the service is not worse after than before
    const rank = { up: 0, degraded: 1, down: 2 }
    expect(rank[p[idx].state]).toBeLessThanOrEqual(rank[p[idx - 1].state])
  })

  it('before the traffic is rerouted some requests still fail', () => {
    const r = recorded('three-az-app').result
    const p = pictures(r.timeline!)
    expect(p[0].state).toBe('degraded')
    expect(p[p.length - 1].state).toBe(r.state)
    expect(r.state).toBe('up')
  })

  it('have no story when nothing takes time and the engine does not tell where the traffic goes', () => {
    const flat = { state: 'up' as const, capacityRatio: null, nodeStates: {}, edgeStates: {} }
    expect(stepsOf({ ...flat, events: [] })).toEqual([])
    expect(stepsOf({ ...flat, events: [], timeline: [{ at: 0, state: 'up', capacityRatio: null, nodes: {}, edges: {} }] })).toEqual([])
    expect(pictures([])).toEqual([])
    expect(story({ ...flat })).toEqual([])
  })
})

describe('the story of a failure', () => {
  const { result } = recorded('two-dc-zone')

  it('starts with normal operation: every node up, every link working, the traffic as it goes without the failure', () => {
    const [before, failure, ...rest] = story(result)
    expect(before.kind).toBe('before')
    expect(Object.values(before.nodes).every((s) => s === 'up')).toBe(true)
    expect(Object.values(before.edges).every((s) => s === 'ok')).toBe(true)
    expect(Object.keys(before.nodes).sort()).toEqual(Object.keys(result.nodeStates).sort())
    expect(before.traffic).toEqual(result.trafficBefore)
    expect(before.state).toBe('up')
    expect(failure.kind).toBe('failure')
    expect(failure.traffic).toEqual(result.timeline![0].traffic)
    expect(rest.every((p) => p.kind === 'step')).toBe(true)
    expect(story(result).at(-1)!.traffic).toEqual(result.traffic)
  })

  it('has the nodes the failure itself breaks pulse at its moment', () => {
    const failure = story(result)[1]
    const broken = Object.entries(result.timeline![0].nodes).filter(([, s]) => s !== 'up').map(([id]) => id)
    expect(broken.length).toBeGreaterThan(0)
    expect(failure.changed.sort()).toEqual(broken.sort())
  })

  it('exists for a failure that is over at once, as normal operation and the outcome', () => {
    const { timeline: _t, ...flat } = result
    const p = story(flat as SimResult)
    expect(p.map((x) => x.kind)).toEqual(['before', 'failure'])
    expect(p[1].nodes).toEqual(result.nodeStates)
    expect(p[1].traffic).toEqual(result.traffic)
  })

  it('is the timeline alone for a backend that does not tell where the traffic goes', () => {
    const { traffic: _a, trafficBefore: _b, ...old } = result
    const old2 = { ...old, timeline: result.timeline!.map(({ traffic: _x, ...f }) => f) } as SimResult
    const p = story(old2)
    expect(p.map((x) => x.kind)).toEqual(['failure', ...result.timeline!.slice(1).map(() => 'step')])
    expect(p.every((x) => x.traffic === null)).toBe(true)
    expect(story({ ...old, timeline: undefined } as SimResult)).toEqual([])
  })
})

describe('the track', () => {
  it('spaces the steps evenly, whatever the time between them', () => {
    expect(trackPosition(0, 4)).toBe(0)
    expect(trackPosition(3, 4)).toBe(100)
    expect(trackPosition(1, 4)).toBeCloseTo(33.33, 1)
    expect(trackPosition(0, 1)).toBe(0)
  })

  it('maps a click to the nearest step', () => {
    expect(stepAtPercent(0, 4)).toBe(0)
    expect(stepAtPercent(100, 4)).toBe(3)
    expect(stepAtPercent(40, 4)).toBe(1)
    expect(stepAtPercent(60, 4)).toBe(2)
    expect(stepAtPercent(-20, 4)).toBe(0)
    expect(stepAtPercent(180, 4)).toBe(3)
    expect(stepAtPercent(50, 1)).toBe(0)
  })
})

describe('the player', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.useFakeTimers()
  })
  afterEach(() => vi.useRealTimers())

  /** `lead`: the administrator's `editor.failureLeadMs` (0: a failure is shown settled and played on request). */
  function show(name: string, lead = 0) {
    const f = recorded(name)
    const app = useApp()
    app.boot = { editor: { failureLeadMs: lead } } as never
    const editor = useEditor()
    editor.arch = layoutAll(f.architecture, realIndex())
    editor.sim = { scenario: f.scenario, result: f.result, loading: false, error: '' }
    return { editor, playback: usePlayback(), result: f.result, app }
  }

  it('shows a failure settled until somebody plays it', () => {
    const { playback, result } = show('two-dc-zone')
    expect(playback.available).toBe(true)
    expect(playback.index).toBe(playback.last)
    expect(playback.playing).toBe(false)
    expect(playback.nodeStates).toEqual(result.nodeStates)
    expect(playback.clock).toBe(result.timeline![result.timeline!.length - 1].at)
    expect(playback.traffic).toEqual(result.traffic)
  })

  it('plays from normal operation through the moment of the failure to the settled state, a step at a time', () => {
    const { playback, result } = show('two-dc-zone')
    playback.play()
    expect(playback.playing).toBe(true)
    expect(playback.index).toBe(0)
    expect(playback.beforeFailure).toBe(true)
    expect(Object.values(playback.nodeStates).every((s) => s === 'up')).toBe(true)
    expect(playback.traffic).toEqual(result.trafficBefore)
    const seen = [playback.index]
    for (let i = 0; i < 20 && playback.playing; i++) {
      vi.advanceTimersByTime(playback.holdMs(playback.index) + 1)
      if (seen[seen.length - 1] !== playback.index) seen.push(playback.index)
    }
    expect(seen).toEqual(Array.from({ length: playback.last + 1 }, (_, i) => i))
    expect(playback.playing).toBe(false)
    expect(playback.nodeStates).toEqual(result.nodeStates)
  })

  it('tells the story by itself when a failure has just been run, and holds normal operation for the administrator\'s lead', () => {
    const { playback, app, result } = show('two-dc-zone', 2400)
    return vi.waitFor(() => {
      expect(playback.playing).toBe(true)
    }).then(() => {
      expect(app.editorCfg.failureLeadMs).toBe(2400)
      expect(playback.index).toBe(0)
      expect(playback.beforeFailure).toBe(true)
      vi.advanceTimersByTime(2300)
      expect(playback.index, 'normal operation is still on screen').toBe(0)
      vi.advanceTimersByTime(200)
      expect(playback.index, 'the failure strikes').toBe(1)
      expect(playback.traffic).toEqual(result.timeline![0].traffic)
      vi.advanceTimersByTime(60_000)
      expect(playback.index).toBe(playback.last)
      expect(playback.playing).toBe(false)
    })
  })

  it('does not play by itself when the lead is 0', () => {
    const { playback } = show('two-dc-zone', 0)
    return vi.waitFor(() => expect(playback.index).toBe(playback.last)).then(() => {
      vi.advanceTimersByTime(60_000)
      expect(playback.playing).toBe(false)
      expect(playback.index).toBe(playback.last)
    })
  })

  it('plays again from the start when it is over', () => {
    const { playback } = show('two-dc-zone')
    playback.play()
    vi.advanceTimersByTime(120_000)
    expect(playback.index).toBe(playback.last)
    playback.play()
    expect(playback.index).toBe(0)
    expect(playback.playing).toBe(true)
  })

  it('stops when the user takes the time into their own hands', () => {
    const { playback } = show('two-dc-zone')
    playback.play()
    vi.advanceTimersByTime(playback.stepMs + 1)
    playback.seek(0)
    expect(playback.playing).toBe(false)
    expect(playback.index).toBe(0)
    vi.advanceTimersByTime(60_000)
    expect(playback.index, 'nothing moves by itself after a pause').toBe(0)
    playback.step(1)
    expect(playback.index).toBe(1)
    playback.step(-5)
    expect(playback.index).toBe(0)
    playback.step(99)
    expect(playback.index).toBe(playback.last)
  })

  it('goes faster at a higher speed', () => {
    const { playback } = show('two-dc-zone')
    const normal = playback.stepMs
    playback.setSpeed(4)
    expect(playback.stepMs).toBe(normal / 4)
  })

  it('marks what changed in the step on screen', () => {
    const { playback } = show('two-dc-zone')
    playback.seek(0)
    expect(playback.changed.size).toBe(0)
    playback.seek(1)
    expect(playback.changed.size, 'the failure breaks something').toBeGreaterThan(0)
    playback.seek(playback.last)
    expect(playback.changed.size).toBeGreaterThan(0)
    for (const id of playback.changed) expect(Object.keys(playback.current!.nodes)).toContain(id)
  })

  it('has the traffic follow the step: normal operation, the moment of the failure, the outcome', () => {
    const { playback, result } = show('two-dc-zone')
    playback.seek(0)
    expect(playback.traffic).toEqual(result.trafficBefore)
    expect(playback.served).toBe(1)
    playback.seek(1)
    expect(playback.traffic).toEqual(result.timeline![0].traffic)
    expect(playback.served).toBe(0)
    playback.seek(playback.last)
    expect(playback.traffic).toEqual(result.traffic)
    expect(playback.before).toEqual(result.trafficBefore)
  })

  it('knows which cards carry more or less than in normal operation, and which are idle', () => {
    const { playback } = show('two-dc-zone')
    playback.seek(1)
    // the whole load goes to the surviving site: its application gets twice what it got
    expect(playback.loads['app-dc2'].ratio).toBeCloseTo(2, 5)
    expect(playback.loads['ingress-dc2'].ratio).toBeCloseTo(2, 5)
    // the lost site has nothing arriving
    expect(playback.loads['app-dc1'].now).toBe(0)
    playback.seek(0)
    expect(playback.loads).toEqual({})
  })

  it('draws every detour of the story, once, wherever it appears', () => {
    const { playback, result } = show('three-az-app')
    expect(playback.detours.length).toBeGreaterThan(0)
    const keys = playback.detours.map((d) => `${d.from}>${d.to}`)
    expect(new Set(keys).size).toBe(keys.length)
    for (const d of result.traffic!.detours ?? []) expect(keys).toContain(`${d.from}>${d.to}`)
  })

  it('thins the particles out for a big diagram and switches them off at 0', () => {
    const { playback, app } = show('two-dc-zone')
    playback.seek(0)
    expect(playback.particleScale).toBe(1)
    app.boot = { editor: { failureLeadMs: 0, flowParticlesMax: 5 } } as never
    expect(playback.particleScale).toBeGreaterThan(0)
    expect(playback.particleScale).toBeLessThan(1)
    app.boot = { editor: { failureLeadMs: 0, flowParticlesMax: 0 } } as never
    expect(playback.particleScale).toBe(0)
  })

  it('a failure that is over at once still has its two pictures, and without the traffic it has no player and the diagram shows the result as it is', () => {
    setActivePinia(createPinia())
    const f = recorded('two-dc-zone')
    useApp().boot = { editor: { failureLeadMs: 0 } } as never
    const editor = useEditor()
    const { timeline: _t, ...flat } = f.result
    editor.sim = { scenario: f.scenario, result: flat as SimResult, loading: false, error: '' }
    const playback = usePlayback()
    expect(playback.available).toBe(true)
    expect(playback.steps.map((s) => s.kind)).toEqual(['before', 'failure'])
    const { traffic: _a, trafficBefore: _b, ...old } = flat
    editor.sim = { scenario: { type: 'zone', zoneId: 'dc2' }, result: old as SimResult, loading: false, error: '' }
    return vi.waitFor(() => expect(playback.available).toBe(false)).then(() => {
      expect(playback.nodeStates).toEqual(f.result.nodeStates)
      expect(playback.traffic).toBeNull()
      playback.play()
      expect(playback.playing).toBe(false)
    })
  })

  it('shows a different failure settled again', () => {
    const { editor, playback } = show('two-dc-zone')
    playback.seek(0)
    const f = recorded('three-az-app')
    editor.sim = { scenario: f.scenario, result: f.result, loading: false, error: '' }
    return vi.waitFor(() => {
      expect(playback.index).toBe(playback.last)
    })
  })
})
