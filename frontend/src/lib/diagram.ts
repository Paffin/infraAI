// A picture of an architecture as plain data: the project thumbnails and the SVG export are the same scene,
// drawn by two serializers (a Vue template for the thumbnail, a string for the file). No DOM, no canvas.

import type { ArchNode, Architecture, Edge, ProjectThumb, Status, Zone } from '@/api/types'
import { CARD_H, CARD_W, boundsOf, nodeRect, placeMissing, zoneChain, zoneRect, type Rect } from '@/editor/layout'
import { sidesFor, type Side } from '@/editor/flowModel'
import { lt } from '@/i18n'
import { nodeSubtitle, type CatalogIndex } from './catalog'

export type Detail = 'thumb' | 'full'

/** Concrete colours (a standalone SVG has no CSS variables). */
export interface Palette {
  bg: string
  surface: string
  line: string
  text: string
  muted: string
  faint: string
  accent: string
  crit: string
  risk: string
  ok: string
}

/** The shipped design tokens (`styles/tokens.css`); `themePalette` overrides them with what the page actually uses. */
export const DEFAULT_PALETTE: Palette = { bg: '#f5f7fb', surface: '#ffffff', line: '#cbd3e0', text: '#161a24', muted: '#5b6270', faint: '#646d80', accent: '#2e5bec', crit: '#c0262d', risk: '#f3a00c', ok: '#27c25e' }

const TOKENS: Record<keyof Palette, string> = { bg: '--bg', surface: '--surface', line: '--line-strong', text: '--text', muted: '--muted', faint: '--faint', accent: '--accent', crit: '--crit', risk: '--risk-dot', ok: '--ok-dot' }

/** The colours of the running page (an administrator may have changed the theme). */
export function themePalette(): Palette {
  if (typeof document === 'undefined') return DEFAULT_PALETTE
  const css = getComputedStyle(document.documentElement)
  const out = { ...DEFAULT_PALETTE }
  for (const [k, v] of Object.entries(TOKENS) as [keyof Palette, string][]) {
    const value = css.getPropertyValue(v).trim()
    if (value) out[k] = value
  }
  return out
}

export type Shape =
  | { t: 'rect'; x: number; y: number; w: number; h: number; rx: number; fill: string; stroke?: string; sw?: number; dash?: string; opacity?: number }
  | { t: 'path'; d: string; stroke: string; sw: number; dash?: string; opacity?: number }
  | { t: 'poly'; points: string; fill: string; opacity?: number }
  | { t: 'circle'; cx: number; cy: number; r: number; fill: string; stroke?: string; sw?: number }
  | { t: 'text'; x: number; y: number; text: string; size: number; fill: string; weight?: number; anchor?: 'start' | 'middle' | 'end'; mono?: boolean }

export interface Scene {
  x: number
  y: number
  w: number
  h: number
  shapes: Shape[]
}

export const EMPTY_SCENE: Scene = { x: 0, y: 0, w: 1, h: 1, shapes: [] }

export interface SceneOptions {
  detail: Detail
  /** worst open finding per node id: draws the status dot */
  statuses?: Map<string, Status>
  palette?: Palette
}

// ---------------------------------------------------------------------------------------------- helpers

const PAD = 40
const NAME_CHARS = 19
const SUB_CHARS = 21

/** Cut a label to `max` characters (an SVG has no text-overflow). */
export function clip(s: string, max: number): string {
  const chars = [...s]
  return chars.length <= max ? s : chars.slice(0, Math.max(1, max - 1)).join('') + '…'
}

/** Where a connection leaves / enters a card: the middle of the chosen side. */
function anchor(r: Rect, side: Side): { x: number; y: number; dx: number; dy: number } {
  switch (side) {
    case 't': return { x: r.x + r.w / 2, y: r.y, dx: 0, dy: -1 }
    case 'b': return { x: r.x + r.w / 2, y: r.y + r.h, dx: 0, dy: 1 }
    case 'l': return { x: r.x, y: r.y + r.h / 2, dx: -1, dy: 0 }
    default: return { x: r.x + r.w, y: r.y + r.h / 2, dx: 1, dy: 0 }
  }
}

const round = (n: number) => Math.round(n * 10) / 10

/** Curve (or straight line) between two cards, with the arrow head when the connection is directed. */
function edgeShapes(e: Edge, a: Rect, b: Rect, o: { color: string; width: number; dash?: string; directed: boolean; opacity?: number }): Shape[] {
  const [sa, sb] = sidesFor({ x: a.x + a.w / 2, y: a.y + a.h / 2 }, { x: b.x + b.w / 2, y: b.y + b.h / 2 })
  const p = anchor(a, sa)
  const q = anchor(b, sb)
  const reach = Math.max(30, Math.hypot(q.x - p.x, q.y - p.y) * 0.35)
  const straight = (sa === 'r' || sa === 'l') && Math.abs(p.y - q.y) < 2
  const d = straight
    ? `M${round(p.x)},${round(p.y)} L${round(q.x)},${round(q.y)}`
    : `M${round(p.x)},${round(p.y)} C${round(p.x + p.dx * reach)},${round(p.y + p.dy * reach)} ${round(q.x + q.dx * reach)},${round(q.y + q.dy * reach)} ${round(q.x)},${round(q.y)}`
  const out: Shape[] = [{ t: 'path', d, stroke: o.color, sw: o.width, dash: o.dash, opacity: o.opacity }]
  if (o.directed) {
    // the head points along the last bit of the curve: into the card from its side
    const len = 9
    const tip = q
    const back = { x: tip.x + tip.dx * len, y: tip.y + tip.dy * len }
    const nx = -tip.dy
    const ny = tip.dx
    out.push({ t: 'poly', points: `${round(tip.x)},${round(tip.y)} ${round(back.x + nx * 4.5)},${round(back.y + ny * 4.5)} ${round(back.x - nx * 4.5)},${round(back.y - ny * 4.5)}`, fill: o.color, opacity: o.opacity })
  }
  return out
}

// ---------------------------------------------------------------------------------------------- scene

/**
 * Draw an architecture. Cards and zones need coordinates (`placeMissing` first); things without them are left
 * out. `thumb` is the overview (zones, coloured tiles, edges); `full` adds names, subtitles and status dots.
 */
export function buildScene(arch: Architecture, ix: CatalogIndex, o: SceneOptions): Scene {
  const pal = o.palette ?? DEFAULT_PALETTE
  const full = o.detail === 'full'
  const b = boundsOf(arch)
  if (!b) return EMPTY_SCENE
  const shapes: Shape[] = []

  // zones: outer ones first so that inner ones are drawn over them
  const zones = arch.zones.map((z) => ({ z, r: zoneRect(z), depth: zoneChain(arch, z.id).length })).filter((x): x is { z: Zone; r: Rect; depth: number } => !!x.r).sort((p, q) => p.depth - q.depth)
  for (const { z, r } of zones) {
    shapes.push({ t: 'rect', x: r.x, y: r.y, w: r.w, h: r.h, rx: 14, fill: pal.surface, opacity: 0.55, stroke: pal.line, sw: full ? 1.2 : 2.5 })
    if (full) {
      shapes.push({ t: 'text', x: r.x + 18, y: r.y + 30, text: clip(z.name, 40), size: 14, weight: 650, fill: pal.text })
      const def = ix.zoneTypes.get(z.type)
      if (def) shapes.push({ t: 'text', x: r.x + 18, y: r.y + 46, text: clip(lt(def.name), 40), size: 11, fill: pal.faint, mono: true })
    }
  }

  // connections
  const byId = new Map(arch.nodes.map((n) => [n.id, n]))
  for (const e of arch.edges) {
    const sa = byId.get(e.source)
    const sb = byId.get(e.target)
    const ra = sa && nodeRect(sa)
    const rb = sb && nodeRect(sb)
    if (!ra || !rb) continue
    const def = ix.edgeTypes.get(e.type)
    shapes.push(...edgeShapes(e, ra, rb, { color: def?.style.color ?? pal.muted, width: full ? Math.max(1.4, def?.style.width ?? 1.6) : 3, dash: def?.style.dash ?? undefined, directed: full && def?.directed !== false, opacity: full ? 1 : 0.7 }))
  }

  // cards
  for (const n of arch.nodes) {
    const r = nodeRect(n)
    if (!r) continue
    shapes.push(...cardShapes(n, r, ix, pal, full, o.statuses?.get(n.id)))
  }

  const x = b.x - PAD
  const y = b.y - PAD
  return { x, y, w: b.w + 2 * PAD, h: b.h + 2 * PAD, shapes }
}

function cardShapes(n: ArchNode, r: Rect, ix: CatalogIndex, pal: Palette, full: boolean, status?: Status): Shape[] {
  const kind = ix.kinds.get(n.kind)
  const color = kind?.color ?? '#4c596c'
  if (!full) return [{ t: 'rect', x: r.x, y: r.y, w: CARD_W, h: CARD_H, rx: 10, fill: color }]
  const out: Shape[] = [
    { t: 'rect', x: r.x, y: r.y, w: CARD_W, h: CARD_H, rx: 8, fill: pal.surface, stroke: pal.line, sw: 1 },
    { t: 'rect', x: r.x + 12, y: r.y + 15, w: 34, h: 34, rx: 7, fill: color },
    { t: 'text', x: r.x + 29, y: r.y + 36, text: kind?.abbr ?? '?', size: 10, weight: 700, fill: '#ffffff', anchor: 'middle' },
    { t: 'text', x: r.x + 57, y: r.y + 28, text: clip(n.name, NAME_CHARS), size: 14, weight: 600, fill: pal.text },
  ]
  const sub = nodeSubtitle(ix, n) || (kind ? lt(kind.name) : n.kind)
  out.push({ t: 'text', x: r.x + 57, y: r.y + 46, text: clip(sub, SUB_CHARS), size: 11.5, fill: pal.muted, mono: true })
  if (status === 'critical' || status === 'risk') out.push({ t: 'circle', cx: r.x + CARD_W, cy: r.y, r: 6, fill: status === 'critical' ? pal.crit : pal.risk, stroke: pal.surface, sw: 2 })
  return out
}

// ---------------------------------------------------------------------------------------------- thumbnails

/** The thumbnail data of the project list as a (name-less) architecture; unplaced diagrams are laid out like the editor does. */
export function thumbToArchitecture(t: ProjectThumb, ix: CatalogIndex): Architecture {
  const num = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v)
  const arch: Architecture = {
    schemaVersion: 1, meta: {}, requirements: {},
    zones: t.zones.map((z) => ({ id: z.i, name: '', type: z.t, parentId: z.p, role: null, props: {}, ui: num(z.x) && num(z.y) && num(z.w) && num(z.h) ? { x: z.x, y: z.y, w: z.w, h: z.h } : undefined })),
    nodes: t.nodes.map((n) => ({ id: n.i, kind: n.k, name: '', zoneId: n.z, group: null, role: null, props: {}, ui: num(n.x) && num(n.y) ? { x: n.x, y: n.y } : undefined })),
    edges: t.edges.map(([source, target, type], i) => ({ id: `e${i}`, source, target, type, label: null, props: {} })),
  }
  const placed = arch.nodes.every((n) => n.ui) && arch.zones.every((z) => z.ui)
  return placed ? arch : placeMissing(arch, ix)
}

// ---------------------------------------------------------------------------------------------- SVG text

export function escapeXml(s: string): string {
  return s.replace(/[<>&"']/g, (c) => ({ '<': '&lt;', '>': '&gt;', '&': '&amp;', '"': '&quot;', "'": '&apos;' })[c]!)
}

const FONT = "Inter, 'Segoe UI', system-ui, -apple-system, Roboto, sans-serif"
const MONO = "'JetBrains Mono', ui-monospace, SFMono-Regular, Menlo, Consolas, monospace"

function shapeToSvg(s: Shape): string {
  const op = (v?: number) => (v !== undefined && v < 1 ? ` opacity="${v}"` : '')
  switch (s.t) {
    case 'rect':
      return `<rect x="${round(s.x)}" y="${round(s.y)}" width="${round(s.w)}" height="${round(s.h)}" rx="${s.rx}" fill="${s.fill}"${s.stroke ? ` stroke="${s.stroke}" stroke-width="${s.sw ?? 1}"` : ''}${s.dash ? ` stroke-dasharray="${s.dash}"` : ''}${op(s.opacity)}/>`
    case 'path':
      return `<path d="${s.d}" fill="none" stroke="${s.stroke}" stroke-width="${s.sw}" stroke-linecap="round"${s.dash ? ` stroke-dasharray="${s.dash}"` : ''}${op(s.opacity)}/>`
    case 'poly':
      return `<polygon points="${s.points}" fill="${s.fill}"${op(s.opacity)}/>`
    case 'circle':
      return `<circle cx="${round(s.cx)}" cy="${round(s.cy)}" r="${s.r}" fill="${s.fill}"${s.stroke ? ` stroke="${s.stroke}" stroke-width="${s.sw ?? 1}"` : ''}/>`
    case 'text':
      return `<text x="${round(s.x)}" y="${round(s.y)}" font-size="${s.size}" font-family="${s.mono ? MONO : FONT}"${s.weight ? ` font-weight="${s.weight}"` : ''} fill="${s.fill}"${s.anchor && s.anchor !== 'start' ? ` text-anchor="${s.anchor}"` : ''}>${escapeXml(s.text)}</text>`
  }
}

export interface LegendItem { label: string; color: string; dash?: string }

/** The standalone SVG file: background, title, the diagram, a legend of the connection types in use. */
export function sceneToSvg(scene: Scene, o: { title: string; palette?: Palette; legend?: LegendItem[] } ): string {
  const pal = o.palette ?? DEFAULT_PALETTE
  const legend = o.legend ?? []
  const head = 44
  const foot = legend.length ? 38 : 0
  const x = scene.x
  const y = scene.y - head
  const w = scene.w
  const h = scene.h + head + foot
  const body = scene.shapes.map(shapeToSvg).join('\n    ')
  let cursor = x + PAD
  const legendSvg = legend
    .map((l) => {
      const at = cursor
      cursor += 54 + [...l.label].length * 7 + 26
      return `<g transform="translate(${round(at)},${round(y + h - 22)})"><line x1="0" y1="-4" x2="34" y2="-4" stroke="${l.color}" stroke-width="2.2" stroke-linecap="round"${l.dash ? ` stroke-dasharray="${l.dash}"` : ''}/><text x="44" y="0" font-size="12" font-family="${FONT}" fill="${pal.muted}">${escapeXml(l.label)}</text></g>`
    })
    .join('\n  ')
  return `<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="${round(x)} ${round(y)} ${round(w)} ${round(h)}" width="${round(w)}" height="${round(h)}" role="img" aria-label="${escapeXml(o.title)}">
  <title>${escapeXml(o.title)}</title>
  <rect x="${round(x)}" y="${round(y)}" width="${round(w)}" height="${round(h)}" fill="${pal.bg}"/>
  <text x="${round(x + PAD)}" y="${round(y + 30)}" font-size="18" font-weight="700" font-family="${FONT}" fill="${pal.text}">${escapeXml(o.title)}</text>
  <g>
    ${body}
  </g>
  ${legendSvg}
</svg>
`
}
