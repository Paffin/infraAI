// Playback of a failure: the frames the engine returns (`SimResult.timeline`) turned into pictures and steps.
//
// The engine simulates the failure again for every moment something finishes (a promotion, a reroute, a restore), so a
// frame is not an animation script but the same deterministic result at that moment. The first frame holds every node
// and edge, the next ones only what changed since the previous: a picture is the sum of the frames up to it.

import type { EdgeState, NodeState, ServiceState, SimEvent, SimFrame, SimResult } from '@/api/types'

export interface Picture {
  at: number
  state: ServiceState
  capacityRatio: number | null
  nodes: Record<string, NodeState>
  edges: Record<string, EdgeState>
  /** nodes whose state is not what it was in the previous picture: what just happened */
  changed: string[]
}

/** Every picture of the timeline, each one complete. */
export function pictures(frames: SimFrame[]): Picture[] {
  const out: Picture[] = []
  let nodes: Record<string, NodeState> = {}
  let edges: Record<string, EdgeState> = {}
  frames.forEach((f, i) => {
    nodes = { ...nodes, ...f.nodes }
    edges = { ...edges, ...f.edges }
    out.push({ at: f.at, state: f.state, capacityRatio: f.capacityRatio, nodes, edges, changed: i === 0 ? [] : Object.keys(f.nodes) })
  })
  return out
}

export interface Step {
  index: number
  at: number
  state: ServiceState
  /** what is complete by this moment and was not before; the first step has what happens with the failure itself */
  events: SimEvent[]
}

/** The story in steps: the events of the result, each in the frame of its moment. */
export function stepsOf(res: Pick<SimResult, 'events' | 'timeline'>): Step[] {
  const frames = res.timeline ?? []
  if (frames.length < 2) return []
  const steps: Step[] = frames.map((f, index) => ({ index, at: f.at, state: f.state, events: [] }))
  for (const e of res.events) {
    const at = e.at ?? 0
    let best = 0
    for (let i = 0; i < frames.length; i++) if (Math.abs(frames[i].at - at) < Math.abs(frames[best].at - at)) best = i
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
