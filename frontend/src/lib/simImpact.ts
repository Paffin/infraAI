// What a failure does to the nodes, from the picture on screen: who is out of service, who serves with losses, who took over.
// The cards on the diagram say it one by one; this is the list that says it at once, and that a click takes to the card.

import type { ArchNode, NodeState, Zone } from '@/api/types'

/** `out`: does not serve; `hurt`: serves, with instances lost; `took`: a replica that took over from a lost writer. */
export type Tier = 'out' | 'hurt' | 'took'

/** The tier of each state of a node; a node that works as it does in normal operation is in none. */
export const TIER: Record<NodeState, Tier | null> = { up: null, down: 'out', fenced: 'out', isolated: 'out', stalled: 'out', partial: 'hurt', promoted: 'took' }

/** The order of the states inside a tier: what is lost for good comes before what only waits for something. */
const RANK: Record<NodeState, number> = { down: 0, fenced: 1, isolated: 2, stalled: 3, partial: 4, promoted: 5, up: 6 }

export interface Hit {
  id: string
  name: string
  state: NodeState
  /** the zone the node sits in (its own name; the site is the part before ` · `), when it has one */
  zone: string | null
}

export interface Impact {
  out: Hit[]
  hurt: Hit[]
  took: Hit[]
  /** all the nodes of the diagram, and the ones that are not serving or serve with losses */
  nodes: number
  affected: number
}

export function impactOf(states: Record<string, NodeState>, nodes: ArchNode[], zones: Zone[]): Impact {
  const zoneName = new Map(zones.map((z) => [z.id, z.name]))
  const out: Hit[] = []
  const hurt: Hit[] = []
  const took: Hit[] = []
  for (const n of nodes) {
    const state = states[n.id]
    const tier = state ? TIER[state] : null
    if (!tier) continue
    const hit: Hit = { id: n.id, name: n.name, state, zone: n.zoneId ? (zoneName.get(n.zoneId) ?? null) : null }
    ;(tier === 'out' ? out : tier === 'hurt' ? hurt : took).push(hit)
  }
  const order = (a: Hit, b: Hit) => RANK[a.state] - RANK[b.state] || a.name.localeCompare(b.name)
  out.sort(order)
  hurt.sort(order)
  took.sort(order)
  return { out, hurt, took, nodes: nodes.length, affected: out.length + hurt.length }
}
