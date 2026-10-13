import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Architecture, Scenario, SimResult } from '../src/api/types'
import { pictures, stepAtPercent, stepsOf, trackPosition } from '../src/lib/playback'
import { useEditor } from '../src/stores/editor'
import { usePlayback } from '../src/stores/playback'

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

  it('tell the story in steps: what happens with the failure first, then each step in the frame of its moment', () => {
    const steps = stepsOf(result)
    expect(steps.map((s) => s.at)).toEqual(result.timeline!.map((f) => f.at))
    expect(steps[0].at).toBe(0)
    const timed = result.events.filter((e) => (e.at ?? 0) > 0)
    expect(timed.length).toBeGreaterThan(0)
    for (const e of timed) expect(steps.find((s) => s.at === e.at)!.events).toContain(e)
    for (const e of result.events.filter((x) => x.at === undefined)) expect(steps[0].events).toContain(e)
    // every event is in exactly one step
    expect(steps.flatMap((s) => s.events)).toHaveLength(result.events.length)
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

  it('have no story when nothing takes time', () => {
    expect(stepsOf({ events: [], timeline: undefined })).toEqual([])
    expect(stepsOf({ events: [], timeline: [{ at: 0, state: 'up', capacityRatio: null, nodes: {}, edges: {} }] })).toEqual([])
    expect(pictures([])).toEqual([])
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

  function show(name: string) {
    const f = recorded(name)
    const editor = useEditor()
    editor.sim = { scenario: f.scenario, result: f.result, loading: false, error: '' }
    return { editor, playback: usePlayback(), result: f.result }
  }

  it('shows a failure settled until somebody plays it', () => {
    const { playback, result } = show('two-dc-zone')
    expect(playback.available).toBe(true)
    expect(playback.index).toBe(playback.last)
    expect(playback.playing).toBe(false)
    expect(playback.nodeStates).toEqual(result.nodeStates)
    expect(playback.clock).toBe(result.timeline![result.timeline!.length - 1].at)
  })

  it('plays from the moment of the failure to the settled state, a step at a time', () => {
    const { playback, result } = show('two-dc-zone')
    playback.play()
    expect(playback.playing).toBe(true)
    expect(playback.index).toBe(0)
    const first = playback.nodeStates
    expect(first).not.toEqual(result.nodeStates)
    const seen = [playback.index]
    for (let i = 0; i < 20 && playback.playing; i++) {
      vi.advanceTimersByTime(playback.stepMs + 1)
      if (seen[seen.length - 1] !== playback.index) seen.push(playback.index)
    }
    expect(seen).toEqual(Array.from({ length: playback.last + 1 }, (_, i) => i))
    expect(playback.playing).toBe(false)
    expect(playback.nodeStates).toEqual(result.nodeStates)
  })

  it('plays again from the start when it is over', () => {
    const { playback } = show('two-dc-zone')
    playback.play()
    vi.advanceTimersByTime(60_000)
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
    playback.seek(playback.last)
    expect(playback.changed.size).toBeGreaterThan(0)
    for (const id of playback.changed) expect(Object.keys(playback.current!.nodes)).toContain(id)
  })

  it('a failure that is over at once has no player and the diagram shows the result as it is', () => {
    setActivePinia(createPinia())
    const f = recorded('two-dc-zone')
    const editor = useEditor()
    const { timeline: _t, ...flat } = f.result
    editor.sim = { scenario: f.scenario, result: flat as SimResult, loading: false, error: '' }
    const playback = usePlayback()
    expect(playback.available).toBe(false)
    expect(playback.nodeStates).toEqual(f.result.nodeStates)
    playback.play()
    expect(playback.playing).toBe(false)
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
