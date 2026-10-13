// Playback of a failure: the frames the engine returns (`SimResult.timeline`) turned into pictures and steps.
//
// The engine simulates the failure again for every moment something finishes (a promotion, a reroute, a restore), so a
// frame is not an animation script but the same deterministic result at that moment. The first frame holds every node
// and edge, the next ones only what changed since the previous: a picture is the sum of the frames up to it.
//
// The story starts one picture before the failure: normal operation, with the traffic as it goes then (`SimResult.trafficBefore`).
// Whoever watches a failure first sees what it breaks.

import type { EdgeState, NodeState, ServiceState, SimEvent, SimFrame, SimResult, Traffic } from '@/api/types'

/** `before`: normal operation; `failure`: the moment of the failure; `step`: after something that takes time has finished. */
export type PictureKind = 'before' | 'failure' | 'step'

export interface Picture {
  at: number
  kind: PictureKind
  state: ServiceState
  capacityRatio: number | null
  nodes: Record<string, NodeState>
  edges: Record<string, EdgeState>
  /** nodes whose state is not what it was in the previous picture: what just happened */
  changed: string[]
  /** where the traffic goes in this picture (absent from a backend that does not tell) */
  traffic: Traffic | null
}

/** Every picture of the timeline, each one complete. */
export function pictures(frames: SimFrame[]): Picture[] {
  const out: Picture[] = []
  let nodes: Record<string, NodeState> = {}
  let edges: Record<string, EdgeState> = {}
  frames.forEach((f, i) => {
    nodes = { ...nodes, ...f.nodes }
    edges = { ...edges, ...f.edges }
    out.push({ at: f.at, kind: i === 0 ? 'failure' : 'step', state: f.state, capacityRatio: f.capacityRatio, nodes, edges, changed: i === 0 ? [] : Object.keys(f.nodes), traffic: f.traffic ?? null })
  })
  return out
}

type StoryInput = Pick<SimResult, 'nodeStates' | 'edgeStates' | 'state' | 'capacityRatio'> & Partial<Pick<SimResult, 'timeline' | 'traffic' | 'trafficBefore'>>

/**
 * The pictures of the whole story: normal operation, the moment of the failure, and every step after it. A failure that is over at
 * once has no steps, but still has its two pictures when the engine told where the traffic goes. Without that there is no story
 * to tell unless something takes time.
 */
export function story(res: StoryInput): Picture[] {
  const frames = res.timeline ?? []
  const after: Picture[] = frames.length
    ? pictures(frames)
    : [{ at: 0, kind: 'failure', state: res.state, capacityRatio: res.capacityRatio, nodes: res.nodeStates, edges: res.edgeStates, changed: [], traffic: res.traffic ?? null }]
  if (!res.trafficBefore) return frames.length > 1 ? after : []
  const up = <T extends string>(ids: string[], v: T): Record<string, T> => Object.fromEntries(ids.map((id) => [id, v]))
  const before: Picture = { at: 0, kind: 'before', state: 'up', capacityRatio: null, nodes: up(Object.keys(res.nodeStates), 'up' as NodeState), edges: up(Object.keys(res.edgeStates), 'ok' as EdgeState), changed: [], traffic: res.trafficBefore }
  // what the failure itself breaks pulses at its moment
  after[0] = { ...after[0], changed: Object.entries(after[0].nodes).filter(([, s]) => s !== 'up').map(([id]) => id) }
  return [before, ...after]
}

export interface Step {
  index: number
  at: number
  kind: PictureKind
  state: ServiceState
  /** the share of the requests that complete at this step, when the engine told */
  served: number | null
  /** what is complete by this moment and was not before; the failure's own step has what happens with the failure itself */
  events: SimEvent[]
}

/** The story in steps: the events of the result, each in the picture of its moment. */
export function stepsOf(res: StoryInput & Pick<SimResult, 'events'>): Step[] {
  const pics = story(res)
  if (pics.length < 2) return []
  const steps: Step[] = pics.map((p, index) => ({ index, at: p.at, kind: p.kind, state: p.state, served: p.traffic?.served ?? null, events: [] }))
  const first = pics.findIndex((p) => p.kind !== 'before')
  for (const e of res.events) {
    const at = e.at ?? 0
    let best = first
    for (let i = first; i < pics.length; i++) if (Math.abs(pics[i].at - at) < Math.abs(pics[best].at - at)) best = i
    steps[best].events.push(e)
  }
  return steps
}

/** Where a step sits on the track, 0–100: the steps are spaced evenly (a restore after two hours would squeeze a promotion after thirty seconds into nothing). */
export function trackPosition(index: number, count: number): number {
  return count < 2 ? 0 : (index / (count - 1)) * 100
}

/** The step a click on the track at `percent` (0–100) lands on. */
export function stepAtPercent(percent: number, count: number): number {
  if (count < 2) return 0
  return Math.min(count - 1, Math.max(0, Math.round((percent / 100) * (count - 1))))
}
