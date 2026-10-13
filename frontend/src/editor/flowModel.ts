// Translation of the architecture (absolute canvas coordinates, flat lists) into Vue Flow's model
// (zones are parent nodes with relative children). Objects are reused between rebuilds when nothing
// they depend on changed, so editing a property never makes the canvas re-measure every card.

import type { Edge as FlowEdge, Node as FlowNode } from '@vue-flow/core'
import type { Architecture } from '@/api/types'
import { CARD_H, CARD_W, DEFAULT_ZONE_H, DEFAULT_ZONE_W, zoneChain } from './layout'

export const nodeFlowId = (id: string) => 'n:' + id
export const zoneFlowId = (id: string) => 'z:' + id
export const edgeFlowId = (id: string) => 'e:' + id

export type Side = 't' | 'b' | 'l' | 'r'
export const SIDES: Side[] = ['t', 'b', 'l', 'r']

export function sidesFor(a: { x: number; y: number }, b: { x: number; y: number }): [Side, Side] {
  const dx = b.x - a.x
  const dy = b.y - a.y
  if (Math.abs(dy) < CARD_H * 0.8 || Math.abs(dx) > Math.abs(dy) * 3) return dx >= 0 ? ['r', 'l'] : ['l', 'r']
  return dy >= 0 ? ['b', 't'] : ['t', 'b']
}

type Cache = Map<string, { sig: string; value: FlowNode | FlowEdge }>

export interface FlowModel { nodes: FlowNode[]; edges: FlowEdge[] }

export function createFlowBuilder() {
  const cache: Cache = new Map()
  const reuse = <T extends FlowNode | FlowEdge>(id: string, sig: string, make: () => T): T => {
    const hit = cache.get(id)
    if (hit && hit.sig === sig) return hit.value as T
    const value = make()
    cache.set(id, { sig, value })
    return value
  }

  return function build(arch: Architecture, draggable: boolean): FlowModel {
    const used = new Set<string>()
    const zones = new Map(arch.zones.map((z) => [z.id, z]))
    const zoneAbs = (id: string | null | undefined) => {
      const z = id ? zones.get(id) : undefined
      return { x: z?.ui?.x ?? 0, y: z?.ui?.y ?? 0 }
    }
    const nodes: FlowNode[] = []

    const ordered = arch.zones.map((z) => ({ z, depth: zoneChain(arch, z.id).length - 1 })).sort((a, b) => a.depth - b.depth)
    for (const { z, depth } of ordered) {
      const id = zoneFlowId(z.id)
      used.add(id)
      const parent = z.parentId && zones.has(z.parentId) ? z.parentId : null
      const p = zoneAbs(parent)
      const x = (z.ui?.x ?? 0) - p.x
      const y = (z.ui?.y ?? 0) - p.y
      const w = z.ui?.w ?? DEFAULT_ZONE_W
      const h = z.ui?.h ?? DEFAULT_ZONE_H
      const sig = `${x}|${y}|${w}|${h}|${parent}|${draggable}|${depth}`
      nodes.push(reuse<FlowNode>(id, sig, () => ({
        id, type: 'zone', position: { x, y }, width: w, height: h, parentNode: parent ? zoneFlowId(parent) : undefined,
        draggable, selectable: false, dragHandle: '.zone-head', zIndex: depth, data: { id: z.id }, connectable: false,
      })))
    }
    for (const n of arch.nodes) {
      const id = nodeFlowId(n.id)
      used.add(id)
      const parent = n.zoneId && zones.has(n.zoneId) ? n.zoneId : null
      const p = zoneAbs(parent)
      const x = (n.ui?.x ?? 0) - p.x
      const y = (n.ui?.y ?? 0) - p.y
      const sig = `${x}|${y}|${parent}|${draggable}`
      nodes.push(reuse<FlowNode>(id, sig, () => ({
        id, type: 'card', position: { x, y }, width: CARD_W, height: CARD_H, parentNode: parent ? zoneFlowId(parent) : undefined,
        draggable, selectable: true, zIndex: 20, data: { id: n.id },
      })))
    }

    const abs = new Map(arch.nodes.map((n) => [n.id, { x: (n.ui?.x ?? 0) + CARD_W / 2, y: (n.ui?.y ?? 0) + CARD_H / 2 }]))
    const pairs = new Map<string, string[]>()
    for (const e of arch.edges) {
      const k = [e.source, e.target].sort().join('|')
      if (!pairs.has(k)) pairs.set(k, [])
      pairs.get(k)!.push(e.id)
    }
    const edges: FlowEdge[] = []
    for (const e of arch.edges) {
      const a = abs.get(e.source)
      const b = abs.get(e.target)
      if (!a || !b) continue
      const id = edgeFlowId(e.id)
      used.add(id)
      const [s, t] = sidesFor(a, b)
      const group = pairs.get([e.source, e.target].sort().join('|'))!
      const lane = group.indexOf(e.id)
      const lanes = group.length
      const sig = `${e.source}|${e.target}|${s}|${t}|${lane}|${lanes}`
      edges.push(reuse<FlowEdge>(id, sig, () => ({
        id, type: 'arch', source: nodeFlowId(e.source), target: nodeFlowId(e.target), sourceHandle: s, targetHandle: t, zIndex: 10, selectable: false, focusable: false,
        data: { id: e.id, lane, lanes, horizontal: s === 'l' || s === 'r' }, interactionWidth: 20,
      })))
    }
    for (const k of [...cache.keys()]) if (!used.has(k)) cache.delete(k)
    return { nodes, edges }
  }
}
