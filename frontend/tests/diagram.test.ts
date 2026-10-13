import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import type { Architecture, ProjectThumb } from '../src/api/types'
import { placeMissing } from '../src/editor/layout'
import { buildScene, clip, escapeXml, sceneToSvg, thumbToArchitecture } from '../src/lib/diagram'
import { fixture, realIndex } from './fixtures'

const ix = realIndex()
const template = (id: string): Architecture => JSON.parse(readFileSync(join(process.cwd(), '..', 'backend', 'crates', 'engine', 'defaults', 'templates', `${id}.json`), 'utf8')).architecture
const placed = (id: string) => placeMissing(template(id), ix)

/** What `GET /projects` ships for the thumbnail of a diagram (see `THUMB_SQL` in the server). */
function thumbOf(a: Architecture): ProjectThumb {
  return {
    zones: a.zones.map((z) => ({ i: z.id, x: z.ui?.x ?? null, y: z.ui?.y ?? null, w: z.ui?.w ?? null, h: z.ui?.h ?? null, t: z.type, p: z.parentId })),
    nodes: a.nodes.map((n) => ({ i: n.id, x: n.ui?.x ?? null, y: n.ui?.y ?? null, k: n.kind, z: n.zoneId })),
    edges: a.edges.map((e) => [e.source, e.target, e.type]),
  }
}

describe('diagram scene', () => {
  it('draws every zone, card and connection of a real reference architecture', () => {
    const a = placed('three-az-region')
    const scene = buildScene(a, ix, { detail: 'full' })
    const rects = scene.shapes.filter((s) => s.t === 'rect')
    const paths = scene.shapes.filter((s) => s.t === 'path')
    // a zone = 1 rect; a card = card rect + badge rect
    expect(rects).toHaveLength(a.zones.length + 2 * a.nodes.length)
    expect(paths).toHaveLength(a.edges.length)
    // every card name is on the picture
    const texts = scene.shapes.flatMap((s) => (s.t === 'text' ? [s.text] : []))
    for (const n of a.nodes) expect(texts, n.name).toContain(clip(n.name, 19))
    // the view box contains all of it
    for (const r of rects) {
      if (r.t !== 'rect') continue
      expect(r.x).toBeGreaterThanOrEqual(scene.x)
      expect(r.y).toBeGreaterThanOrEqual(scene.y)
      expect(r.x + r.w).toBeLessThanOrEqual(scene.x + scene.w)
      expect(r.y + r.h).toBeLessThanOrEqual(scene.y + scene.h)
    }
  })

  it('the overview has no text and one tile per card', () => {
    const a = placed('three-az-region')
    const scene = buildScene(a, ix, { detail: 'thumb' })
    expect(scene.shapes.some((s) => s.t === 'text')).toBe(false)
    expect(scene.shapes.filter((s) => s.t === 'rect')).toHaveLength(a.zones.length + a.nodes.length)
  })

  it('marks cards with open critical findings or risks', () => {
    const { architecture: a, report } = fixture('report-spof')
    const arch = placeMissing(a, ix)
    const statuses = new Map(report.findings.filter((f) => f.status === 'critical').flatMap((f) => f.subjects.nodes.map((n) => [n, f.status] as const)))
    const dots = buildScene(arch, ix, { detail: 'full', statuses }).shapes.filter((s) => s.t === 'circle')
    expect(dots).toHaveLength(statuses.size)
    expect(statuses.size).toBeGreaterThan(0)
  })

  it('an empty architecture is an empty scene', () => {
    const empty: Architecture = { schemaVersion: 1, meta: {}, requirements: {}, zones: [], nodes: [], edges: [] }
    expect(buildScene(empty, ix, { detail: 'thumb' }).shapes).toEqual([])
  })
})

describe('SVG file', () => {
  const svgOf = (a: Architecture, title = 'Shop') => {
    const scene = buildScene(a, ix, { detail: 'full' })
    return sceneToSvg(scene, { title, legend: [{ label: 'sync', color: '#1f9d55' }, { label: 'async', color: '#e08a00', dash: '5 4' }] })
  }

  it('is well-formed XML with the title, the diagram and a legend', () => {
    const svg = svgOf(placed('two-dc-witness'), 'Магазин · prod')
    const doc = new DOMParser().parseFromString(svg, 'image/svg+xml')
    expect(doc.querySelector('parsererror')).toBeNull()
    expect(doc.documentElement.tagName).toBe('svg')
    expect(doc.querySelector('title')!.textContent).toBe('Магазин · prod')
    const a = placed('two-dc-witness')
    expect(doc.querySelectorAll('path').length).toBeGreaterThanOrEqual(a.edges.length)
    expect([...doc.querySelectorAll('text')].map((t) => t.textContent)).toEqual(expect.arrayContaining(['sync', 'async']))
    const [, , w, h] = doc.documentElement.getAttribute('viewBox')!.split(' ').map(Number)
    expect(doc.documentElement.getAttribute('width')).toBe(String(w))
    expect(doc.documentElement.getAttribute('height')).toBe(String(h))
  })

  it('escapes what the user typed (names go into the file)', () => {
    const a = placed('two-dc-witness')
    a.nodes[0].name = `A & <B> "C" 'D'`
    a.zones[0].name = '</text><script>alert(1)</script>'
    const svg = svgOf(a, 'x</title><g>')
    const doc = new DOMParser().parseFromString(svg, 'image/svg+xml')
    expect(doc.querySelector('parsererror')).toBeNull()
    expect(doc.querySelector('script')).toBeNull()
    expect(svg).not.toContain('<script>')
    expect(svg).toContain('A &amp; &lt;B&gt; &quot;C&quot; &apos;D&apos;'.slice(0, 18))
    expect(escapeXml(`<&>"'`)).toBe('&lt;&amp;&gt;&quot;&apos;')
  })

  it('cuts long labels with an ellipsis', () => {
    expect(clip('short', 19)).toBe('short')
    expect(clip('PostgreSQL primary in the second data center', 19)).toHaveLength(19)
    expect(clip('PostgreSQL primary in the second data center', 19).endsWith('…')).toBe(true)
    expect([...clip('Ёмкость кластера Kubernetes для приложения', 10)]).toHaveLength(10)
  })
})

describe('project thumbnails', () => {
  it('a laid-out diagram keeps its coordinates', () => {
    const a = placed('three-az-region')
    const back = thumbToArchitecture(thumbOf(a), ix)
    expect(back.nodes.map((n) => n.ui)).toEqual(a.nodes.map((n) => ({ x: n.ui!.x, y: n.ui!.y })))
    expect(back.zones.map((z) => z.ui)).toEqual(a.zones.map((z) => ({ x: z.ui!.x, y: z.ui!.y, w: z.ui!.w, h: z.ui!.h })))
    expect(back.edges).toHaveLength(a.edges.length)
  })

  it('a diagram the editor has not laid out yet is laid out for the thumbnail', () => {
    const raw = template('multi-region-pilot-light')
    expect(raw.nodes.every((n) => !n.ui)).toBe(true)
    const t = thumbOf(raw)
    expect(t.nodes.every((n) => n.x === null)).toBe(true)
    const arch = thumbToArchitecture(t, ix)
    expect(arch.nodes.every((n) => typeof n.ui?.x === 'number' && typeof n.ui?.y === 'number')).toBe(true)
    expect(arch.zones.every((z) => !!z.ui?.w)).toBe(true)
    const scene = buildScene(arch, ix, { detail: 'thumb' })
    expect(scene.shapes.filter((s) => s.t === 'rect').length).toBe(raw.zones.length + raw.nodes.length)
    // no two cards on top of each other
    const cards = arch.nodes.map((n) => n.ui!)
    for (let i = 0; i < cards.length; i++) for (let j = i + 1; j < cards.length; j++) {
      const overlap = Math.abs(cards[i].x! - cards[j].x!) < 212 && Math.abs(cards[i].y! - cards[j].y!) < 64
      expect(overlap, `${arch.nodes[i].id} / ${arch.nodes[j].id}`).toBe(false)
    }
  })
})
