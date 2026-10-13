// Deterministic layout of an architecture on the canvas.
//
// Nodes sit on a grid of rows (one row per "depth" along the dependency edges), zones are columns that
// span the same rows, so equivalent components of different sites line up (WAF above WAF, database above
// database) like on a hand-drawn diagram. Global (zone-less) components form a band above the sites
// (entry points) or below them. The result is plain data (`ui` on nodes and zones, absolute canvas
// coordinates) — the user can drag everything afterwards.

import type { ArchNode, Architecture, Ui, Zone } from '@/api/types'
import type { CatalogIndex } from '@/lib/catalog'

export const CARD_W = 212
export const CARD_H = 64
const H_GAP = 56
const V_GAP = 62
const PAD_X = 28
const PAD_TOP = 58
const PAD_BOTTOM = 26
const NEST = 44
const ZONE_GAP = 48
const BAND_GAP = 84
const MAX_PER_ROW = 2
export const MIN_ZONE_W = CARD_W + 2 * PAD_X
export const MIN_ZONE_H = 190
export const DEFAULT_ZONE_W = 520
export const DEFAULT_ZONE_H = 420

export interface Rect { x: number; y: number; w: number; h: number }

export const hasPos = (u?: Ui | null): u is Ui & { x: number; y: number } => !!u && Number.isFinite(u.x) && Number.isFinite(u.y)
export const hasRect = (u?: Ui | null): u is Required<Ui> => hasPos(u) && Number.isFinite(u!.w) && Number.isFinite(u!.h) && (u!.w as number) > 0 && (u!.h as number) > 0

export function nodeRect(n: ArchNode): Rect | null {
  return hasPos(n.ui) ? { x: n.ui.x!, y: n.ui.y!, w: CARD_W, h: CARD_H } : null
}

export function zoneRect(z: Zone): Rect | null {
  return hasRect(z.ui) ? { x: z.ui!.x!, y: z.ui!.y!, w: z.ui!.w!, h: z.ui!.h! } : null
}

const contains = (r: Rect, x: number, y: number) => x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h

const clone = <T>(x: T): T => JSON.parse(JSON.stringify(x))

// ---------------------------------------------------------------------------------------- depth

function dependencyEdges(arch: Architecture, ix: CatalogIndex) {
  const ids = new Set(arch.nodes.map((n) => n.id))
  return arch.edges.filter((e) => e.source !== e.target && ids.has(e.source) && ids.has(e.target) && (ix.edgeTypes.get(e.type)?.semantics ?? 'dependency') === 'dependency')
}

/** Longest-path depth along dependency edges (back edges of cycles ignored); `null` = no dependency edges. */
export function computeDepths(arch: Architecture, ix: CatalogIndex): Map<string, number | null> {
  const deps = dependencyEdges(arch, ix)
  const out = new Map<string, string[]>()
  const hasDep = new Set<string>()
  for (const e of deps) {
    if (!out.has(e.source)) out.set(e.source, [])
    out.get(e.source)!.push(e.target)
    hasDep.add(e.source)
    hasDep.add(e.target)
  }

  // break cycles: iterative DFS, edges pointing at a node on the current path are dropped
  const state = new Map<string, 0 | 1 | 2>()
  const back = new Set<string>()
  for (const n of arch.nodes) {
    if (state.get(n.id) || !hasDep.has(n.id)) continue
    const stack: [string, number][] = [[n.id, 0]]
    state.set(n.id, 1)
    while (stack.length) {
      const top = stack[stack.length - 1]
      const kids = out.get(top[0]) ?? []
      if (top[1] >= kids.length) { state.set(top[0], 2); stack.pop(); continue }
      const k = kids[top[1]++]
      const s = state.get(k)
      if (s === 1) back.add(top[0] + '>' + k)
      else if (!s) { state.set(k, 1); stack.push([k, 0]) }
    }
  }

  const indeg = new Map<string, number>()
  const fwd = new Map<string, string[]>()
  for (const e of deps) {
    if (back.has(e.source + '>' + e.target)) continue
    if (!fwd.has(e.source)) fwd.set(e.source, [])
    fwd.get(e.source)!.push(e.target)
    indeg.set(e.target, (indeg.get(e.target) ?? 0) + 1)
  }
  const depth = new Map<string, number | null>()
  for (const n of arch.nodes) depth.set(n.id, hasDep.has(n.id) ? 0 : null)
  const queue = arch.nodes.filter((n) => hasDep.has(n.id) && !(indeg.get(n.id) ?? 0)).map((n) => n.id)
  while (queue.length) {
    const u = queue.shift()!
    for (const v of fwd.get(u) ?? []) {
      depth.set(v, Math.max(depth.get(v) ?? 0, (depth.get(u) ?? 0) + 1))
      indeg.set(v, (indeg.get(v) ?? 1) - 1)
      if (!indeg.get(v)) queue.push(v)
    }
  }

  // replicas, backups and quorum members share the row of the component they belong to; platform
  // components that only have control edges (monitoring, secrets, GitOps) end up in the bottom row
  const near = new Set(['replication', 'backup', 'membership'])
  const others = arch.edges.filter((e) => e.source !== e.target && !deps.includes(e) && near.has(ix.edgeTypes.get(e.type)?.semantics ?? ''))
  for (let pass = 0; pass < 8; pass++) {
    let changed = false
    for (const n of arch.nodes) {
      if (depth.get(n.id) !== null) continue
      let best: number | null = null
      for (const e of others) {
        const other = e.source === n.id ? e.target : e.target === n.id ? e.source : null
        const d = other ? depth.get(other) : null
        if (d != null && (best === null || d < best)) best = d
      }
      if (best !== null) { depth.set(n.id, best); changed = true }
    }
    if (!changed) break
  }
  return depth
}

// ---------------------------------------------------------------------------------------- layout

interface Block {
  zone: Zone
  nodes: Map<number, ArchNode[]>
  kids: Block[]
  directW: number
  w: number
  r0: number | null
  r1: number | null
  levels: number
}

export function layoutAll(input: Architecture, ix: CatalogIndex): Architecture {
  const arch = clone(input)
  if (!arch.nodes.length && !arch.zones.length) return arch

  const depth = computeDepths(arch, ix)
  const zoneIds = new Set(arch.zones.map((z) => z.id))
  const known = [...depth.values()].filter((d): d is number => d !== null)
  const isolatedDepth = (known.length ? Math.max(...known) : 0) + 1
  const d = (n: ArchNode) => depth.get(n.id) ?? isolatedDepth

  const catOrder = new Map(ix.catalog.categories.map((c) => [c.id, c.order]))
  const kindOrder = (n: ArchNode) => (catOrder.get(ix.kinds.get(n.kind)?.category ?? '') ?? 99) * 1000 + [...ix.kinds.keys()].indexOf(n.kind)
  const order = new Map(arch.nodes.map((n, i) => [n.id, i]))
  const sortNodes = (a: ArchNode, b: ArchNode) => kindOrder(a) - kindOrder(b) || order.get(a.id)! - order.get(b.id)!

  const zoned = arch.nodes.filter((n) => n.zoneId && zoneIds.has(n.zoneId))
  const free = arch.nodes.filter((n) => !(n.zoneId && zoneIds.has(n.zoneId)))
  const minZoned = zoned.length ? Math.min(...zoned.map(d)) : Infinity
  const topNodes = free.filter((n) => d(n) < minZoned)
  const bottomNodes = free.filter((n) => d(n) >= minZoned)

  // compact depths into row numbers per band
  const rowsOf = (nodes: ArchNode[]) => {
    const ds = [...new Set(nodes.map(d))].sort((a, b) => a - b)
    return new Map(ds.map((v, i) => [v, i]))
  }
  // rows of the zone band: one block of rows per depth; a depth gets as many rows as the fullest zone needs
  const perCell = new Map<string, number>()
  for (const n of zoned) perCell.set(`${n.zoneId}|${d(n)}`, (perCell.get(`${n.zoneId}|${d(n)}`) ?? 0) + 1)
  const rowsAtDepth = new Map<number, number>()
  for (const [key, count] of perCell) {
    const depthKey = Number(key.slice(key.lastIndexOf('|') + 1))
    rowsAtDepth.set(depthKey, Math.max(rowsAtDepth.get(depthKey) ?? 1, Math.ceil(count / MAX_PER_ROW)))
  }
  const rowStart = new Map<number, number>()
  let nextRow = 0
  for (const depthKey of [...rowsAtDepth.keys()].sort((a, b) => a - b)) {
    rowStart.set(depthKey, nextRow)
    nextRow += rowsAtDepth.get(depthKey)!
  }
  const topRows = rowsOf(topNodes)
  const bottomRows = rowsOf(bottomNodes)

  // zone tree
  const byParent = new Map<string | null, Zone[]>()
  for (const z of arch.zones) {
    const p = z.parentId && zoneIds.has(z.parentId) && z.parentId !== z.id ? z.parentId : null
    if (!byParent.has(p)) byParent.set(p, [])
    byParent.get(p)!.push(z)
  }
  const seen = new Set<string>()
  const build = (z: Zone, mirror: boolean): Block => {
    seen.add(z.id)
    const nodes = new Map<number, ArchNode[]>()
    const byDepth = new Map<number, ArchNode[]>()
    for (const n of zoned.filter((n) => n.zoneId === z.id).sort(sortNodes)) {
      if (!byDepth.has(d(n))) byDepth.set(d(n), [])
      byDepth.get(d(n))!.push(n)
    }
    for (const [depthKey, list] of byDepth) {
      list.forEach((n, j) => {
        const r = rowStart.get(depthKey)! + Math.floor(j / MAX_PER_ROW)
        if (!nodes.has(r)) nodes.set(r, [])
        nodes.get(r)!.push(n)
      })
    }
    // sibling zones alternate their left/right order so that the same component of neighbouring sites
    // (database, queue) sits next to each other across the border and replication links stay short
    if (mirror) for (const list of nodes.values()) list.reverse()
    const kids = (byParent.get(z.id) ?? []).filter((c) => !seen.has(c.id)).map((c, i) => build(c, i % 2 === 0))
    const directW = nodes.size ? Math.max(...[...nodes.values()].map((l) => l.length)) * (CARD_W + H_GAP) - H_GAP : 0
    const slots = (directW ? 1 : 0) + kids.length
    const innerW = directW + kids.reduce((s, k) => s + k.w, 0) + Math.max(0, slots - 1) * H_GAP
    const rows = [...nodes.keys(), ...kids.flatMap((k) => (k.r0 === null ? [] : [k.r0, k.r1!]))]
    return {
      zone: z, nodes, kids, directW,
      w: Math.max(MIN_ZONE_W, innerW + 2 * PAD_X),
      r0: rows.length ? Math.min(...rows) : null,
      r1: rows.length ? Math.max(...rows) : null,
      levels: kids.length ? 1 + Math.max(...kids.map((k) => k.levels)) : 0,
    }
  }
  const roots = (byParent.get(null) ?? []).map((z, i) => build(z, i % 2 === 0))
  // zones that are part of a parent cycle: promote to roots
  for (const z of arch.zones) if (!seen.has(z.id)) roots.push(build(z, false))

  const rowStep = CARD_H + V_GAP
  const withContent = roots.filter((b) => b.r0 !== null)
  const R0 = withContent.length ? Math.min(...withContent.map((b) => b.r0!)) : 0
  const R1 = withContent.length ? Math.max(...withContent.map((b) => b.r1!)) : 0
  const rootLevels = roots.length ? Math.max(...roots.map((b) => b.levels)) : 0

  // horizontal extent of the zone band decides where the global nodes are centred
  const zonesW = roots.reduce((s, b) => s + b.w, 0) + Math.max(0, roots.length - 1) * ZONE_GAP
  const rowWidth = (n: number) => n * (CARD_W + H_GAP) - H_GAP
  const bandW = (nodes: ArchNode[], rows: Map<number, number>) => {
    const counts = new Map<number, number>()
    for (const n of nodes) counts.set(rows.get(d(n))!, (counts.get(rows.get(d(n))!) ?? 0) + 1)
    return counts.size ? Math.max(...[...counts.values()].map(rowWidth)) : 0
  }
  const totalW = Math.max(zonesW, bandW(topNodes, topRows), bandW(bottomNodes, bottomRows))
  const cx = totalW / 2

  const placeBand = (nodes: ArchNode[], rows: Map<number, number>, y0: number) => {
    const grouped = new Map<number, ArchNode[]>()
    for (const n of nodes.sort(sortNodes)) {
      const r = rows.get(d(n))!
      if (!grouped.has(r)) grouped.set(r, [])
      grouped.get(r)!.push(n)
    }
    for (const [r, list] of grouped) {
      const w = rowWidth(list.length)
      list.forEach((n, i) => { n.ui = { ...n.ui, x: Math.round(cx - w / 2 + i * (CARD_W + H_GAP)), y: y0 + r * rowStep } })
    }
    return grouped.size ? y0 + (grouped.size - 1) * rowStep + CARD_H : y0
  }

  const topBottom = placeBand(topNodes, topRows, 0)
  const hasTop = topNodes.length > 0
  const bandY0 = (hasTop ? topBottom + BAND_GAP : 0) + PAD_TOP + NEST * rootLevels
  const rowY = (r: number) => bandY0 + (r - R0) * rowStep

  const place = (b: Block, x: number, topLevels: number | null) => {
    const lv = topLevels ?? b.levels
    const r0 = topLevels !== null ? R0 : b.r0 ?? R0
    const r1 = topLevels !== null ? R1 : b.r1 ?? R0
    const y = rowY(r0) - PAD_TOP - NEST * lv
    const bottom = rowY(r1) + CARD_H + PAD_BOTTOM + (NEST / 2) * lv
    b.zone.ui = { ...b.zone.ui, x: Math.round(x), y: Math.round(y), w: Math.round(b.w), h: Math.round(Math.max(MIN_ZONE_H, bottom - y)) }
    const slots = (b.directW ? 1 : 0) + b.kids.length
    const innerW = b.directW + b.kids.reduce((s, k) => s + k.w, 0) + Math.max(0, slots - 1) * H_GAP
    let cursor = x + PAD_X + (b.w - 2 * PAD_X - innerW) / 2
    if (b.directW) {
      for (const [r, list] of b.nodes) {
        const w = rowWidth(list.length)
        list.forEach((n, i) => { n.ui = { ...n.ui, x: Math.round(cursor + (b.directW - w) / 2 + i * (CARD_W + H_GAP)), y: Math.round(rowY(r)) } })
      }
      cursor += b.directW + H_GAP
    }
    for (const k of b.kids) { place(k, cursor, null); cursor += k.w + H_GAP }
  }
  let x = Math.round((totalW - zonesW) / 2)
  for (const b of roots) { place(b, x, b.r0 !== null || roots.length ? rootLevels : null); x += b.w + ZONE_GAP }

  const zoneBottom = roots.length ? Math.max(...roots.map((b) => (b.zone.ui?.y ?? 0) + (b.zone.ui?.h ?? 0))) : topBottom
  placeBand(bottomNodes, bottomRows, zoneBottom + BAND_GAP)
  return arch
}

// ------------------------------------------------------------------------------ incremental placement

export function boundsOf(arch: Architecture): Rect | null {
  const rects = [...arch.nodes.map(nodeRect), ...arch.zones.map(zoneRect)].filter((r): r is Rect => !!r)
  if (!rects.length) return null
  const x0 = Math.min(...rects.map((r) => r.x))
  const y0 = Math.min(...rects.map((r) => r.y))
  const x1 = Math.max(...rects.map((r) => r.x + r.w))
  const y1 = Math.max(...rects.map((r) => r.y + r.h))
  return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
}

const overlaps = (a: Rect, b: Rect, m = 10) => a.x < b.x + b.w + m && a.x + a.w + m > b.x && a.y < b.y + b.h + m && a.y + a.h + m > b.y

/** Give every node/zone without coordinates a position, keeping everything that already has one. */
export function placeMissing(input: Architecture, ix: CatalogIndex): Architecture {
  const missingNodes = input.nodes.filter((n) => !hasPos(n.ui))
  const missingZones = input.zones.filter((z) => !hasRect(z.ui))
  if (!missingNodes.length && !missingZones.length) return input
  if (missingNodes.length === input.nodes.length && missingZones.length === input.zones.length) return layoutAll(input, ix)

  const arch = clone(input)
  const bounds = boundsOf(arch)
  let nextZoneX = bounds ? bounds.x + bounds.w + ZONE_GAP : 0
  const zoneTop = arch.zones.filter((z) => hasRect(z.ui)).map((z) => z.ui!.y!)
  const zoneY = zoneTop.length ? Math.min(...zoneTop) : bounds ? bounds.y : 0
  const zoneH = Math.max(MIN_ZONE_H, ...arch.zones.filter((z) => hasRect(z.ui)).map((z) => z.ui!.h!))
  for (const z of arch.zones) {
    if (hasRect(z.ui)) continue
    z.ui = { x: nextZoneX, y: zoneY, w: DEFAULT_ZONE_W, h: zoneH }
    nextZoneX += DEFAULT_ZONE_W + ZONE_GAP
  }

  const nodeSlot = (n: ArchNode) => {
    const zone = n.zoneId ? arch.zones.find((z) => z.id === n.zoneId) : undefined
    const placed = arch.nodes.filter((o) => o !== n && hasPos(o.ui)).map((o) => nodeRect(o)!)
    const childZones = zone ? arch.zones.filter((c) => c.parentId === zone.id).map((c) => zoneRect(c)!).filter(Boolean) : []
    const occupied = [...placed, ...childZones]
    if (zone && hasRect(zone.ui)) {
      const zr = zoneRect(zone)!
      for (let guard = 0; guard < 200; guard++) {
        for (let y = zr.y + PAD_TOP; y + CARD_H <= zr.y + zr.h - PAD_BOTTOM; y += CARD_H + V_GAP) {
          for (let x = zr.x + PAD_X; x + CARD_W <= zr.x + zr.w - PAD_X; x += CARD_W + H_GAP) {
            if (!occupied.some((o) => overlaps({ x, y, w: CARD_W, h: CARD_H }, o))) return { x, y }
          }
        }
        zone.ui!.h = (zone.ui!.h ?? zr.h) + CARD_H + V_GAP
        zr.h = zone.ui!.h!
      }
    }
    const b = boundsOf(arch)
    return b ? { x: b.x + ((arch.nodes.filter((o) => !zone && hasPos(o.ui)).length * (CARD_W + H_GAP)) % Math.max(CARD_W + H_GAP, b.w)), y: b.y + b.h + BAND_GAP } : { x: 0, y: 0 }
  }
  for (const n of arch.nodes) if (!hasPos(n.ui)) n.ui = { ...n.ui, ...nodeSlot(n) }
  return arch
}

// ---------------------------------------------------------------------------------------- queries

/** Zone ids from the given zone up to the root. */
export function zoneChain(arch: Architecture, zoneId: string | null | undefined): string[] {
  const out: string[] = []
  const byId = new Map(arch.zones.map((z) => [z.id, z]))
  let cur = zoneId ? byId.get(zoneId) : undefined
  while (cur && !out.includes(cur.id) && out.length < 64) {
    out.push(cur.id)
    cur = cur.parentId ? byId.get(cur.parentId) : undefined
  }
  return out
}

export function zoneDescendants(arch: Architecture, zoneId: string): { zones: Set<string>; nodes: Set<string> } {
  const zones = new Set<string>([zoneId])
  let grew = true
  while (grew) {
    grew = false
    for (const z of arch.zones) if (z.parentId && zones.has(z.parentId) && !zones.has(z.id)) { zones.add(z.id); grew = true }
  }
  return { zones, nodes: new Set(arch.nodes.filter((n) => n.zoneId && zones.has(n.zoneId)).map((n) => n.id)) }
}

/** Innermost zone whose rectangle contains the point (optionally ignoring a zone subtree, e.g. the dragged one). */
export function zoneAt(arch: Architecture, x: number, y: number, ignore?: Set<string>): Zone | null {
  let best: Zone | null = null
  let bestArea = Infinity
  for (const z of arch.zones) {
    if (ignore?.has(z.id)) continue
    const r = zoneRect(z)
    if (r && contains(r, x, y) && r.w * r.h < bestArea) { best = z; bestArea = r.w * r.h }
  }
  return best
}

/** Place `add` to the right of `base`; ids and groups that collide get a common `-N` suffix. */
export function mergeArchitecture(base: Architecture, add: Architecture, ix: CatalogIndex): Architecture {
  const merged = clone(base)
  const laid = layoutAll(add, ix)
  const taken = new Set([...base.nodes.map((n) => n.id), ...base.zones.map((z) => z.id), ...base.edges.map((e) => e.id)])
  const groups = new Set(base.nodes.map((n) => n.group).filter(Boolean) as string[])
  const ids = [...laid.zones.map((z) => z.id), ...laid.nodes.map((n) => n.id), ...laid.edges.map((e) => e.id)]
  const gs = laid.nodes.map((n) => n.group).filter(Boolean) as string[]
  let tag = ''
  if (ids.some((id) => taken.has(id)) || gs.some((g) => groups.has(g))) {
    let i = 2
    while (ids.some((id) => taken.has(`${id}-${i}`)) || gs.some((g) => groups.has(`${g}-${i}`))) i++
    tag = `-${i}`
  }
  const b = boundsOf(base)
  const shiftX = b ? b.x + b.w + ZONE_GAP * 2 : 0
  const shiftY = b ? b.y : 0
  for (const z of laid.zones) {
    merged.zones.push({ ...z, id: z.id + tag, parentId: z.parentId ? z.parentId + tag : null, ui: { ...z.ui, x: z.ui!.x! + shiftX, y: z.ui!.y! + shiftY } })
  }
  for (const n of laid.nodes) {
    merged.nodes.push({ ...n, id: n.id + tag, zoneId: n.zoneId ? n.zoneId + tag : null, group: n.group ? n.group + tag : null, ui: { ...n.ui, x: n.ui!.x! + shiftX, y: n.ui!.y! + shiftY } })
  }
  for (const e of laid.edges) merged.edges.push({ ...e, id: e.id + tag, source: e.source + tag, target: e.target + tag })
  return merged
}

/**
 * Move nodes into a zone: they take free slots of the zone's grid (the cards that stay are left alone) and the
 * zone grows downwards when its grid is full. Pure: returns a new architecture.
 */
export function placeInZone(input: Architecture, nodeIds: string[], zoneId: string): Architecture {
  const arch = clone(input)
  const zone = arch.zones.find((z) => z.id === zoneId)
  const zr = zone && zoneRect(zone)
  if (!zone || !zr) return arch
  const moving = new Set(nodeIds)
  const taken: Rect[] = arch.nodes.filter((n) => !moving.has(n.id)).map(nodeRect).filter((r): r is Rect => !!r)
  const cols = Math.max(1, Math.floor((zr.w - 2 * PAD_X + H_GAP) / (CARD_W + H_GAP)))
  const slot = (i: number): Rect => ({ x: zr.x + PAD_X + (i % cols) * (CARD_W + H_GAP), y: zr.y + PAD_TOP + Math.floor(i / cols) * (CARD_H + V_GAP), w: CARD_W, h: CARD_H })
  let i = 0
  let bottom = zr.y + zr.h
  for (const n of arch.nodes) {
    if (!moving.has(n.id)) continue
    let r = slot(i)
    while (taken.some((t) => overlaps(t, r))) r = slot(++i)
    n.zoneId = zoneId
    n.ui = { ...n.ui, x: r.x, y: r.y }
    taken.push(r)
    i++
    bottom = Math.max(bottom, r.y + r.h + PAD_BOTTOM)
  }
  if (bottom > zr.y + zr.h && zone.ui) zone.ui = { ...zone.ui, h: bottom - zr.y }
  return arch
}
