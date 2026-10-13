import { beforeAll, describe, expect, it } from 'vitest'
import { setLang } from '../src/i18n'
import { realPlacement } from './fixtures'
import {
  CONFIG_KINDS, itemMatches, itemTitle, kindFromRoute, kindPath, normalizeSection, orderedForEditor, placeholdersIn, promptVars,
  skeleton, unknownPlaceholders, validItemId, withId,
} from '../src/admin/lib/configKinds'

const lt = (x: unknown, l = 'ru') => (typeof x === 'string' ? x : x && typeof x === 'object' ? ((x as Record<string, string>)[l] ?? (x as Record<string, string>).en ?? '') : '')

describe('routing', () => {
  it('maps route params to kinds', () => {
    expect(kindFromRoute('catalog', undefined)?.kind).toBe('catalog.kind')
    expect(kindFromRoute('catalog', 'edge_type')?.kind).toBe('catalog.edge_type')
    expect(kindFromRoute('catalog', 'nope')).toBeUndefined()
    expect(kindFromRoute('rules', undefined)?.kind).toBe('rule')
    expect(kindFromRoute('engine', undefined)?.single).toBe(true)
    expect(kindFromRoute('users', undefined)).toBeUndefined()
  })

  it('opens the first page of a section with several pages', () => {
    expect(kindFromRoute('placement', undefined)?.kind).toBe('placement.pattern')
    expect(kindFromRoute('placement', 'stack')?.kind).toBe('placement.stack')
    expect(kindFromRoute('placement', 'settings')?.single).toBe(true)
    expect(kindFromRoute('placement', 'nope')).toBeUndefined()
  })

  it('builds links to items', () => {
    expect(kindPath('rule', 'HA-SPOF-01')).toEqual({ path: '/admin/rules', query: { item: 'HA-SPOF-01' } })
    expect(kindPath('catalog.zone_type')).toEqual({ path: '/admin/catalog/zone_type' })
    for (const k of CONFIG_KINDS) expect(kindFromRoute(k.section, k.sub)?.kind).toBe(k.kind)
  })

  it('falls back to settings for unknown sections', () => {
    expect(normalizeSection('audit')).toBe('audit')
    expect(normalizeSection('nope')).toBe('settings')
    expect(normalizeSection(undefined)).toBe('settings')
  })
})

describe('itemTitle', () => {
  beforeAll(() => setLang('en'))

  it('derives the title from name or title with a language fallback', () => {
    expect(itemTitle('rule', 'R1', { title: { ru: 'Правило', en: 'Rule' }, category: 'ha', check: 'spof' }, (x) => lt(x, 'ru'))).toEqual({ title: 'Правило', subtitle: 'ha · spof' })
    expect(itemTitle('catalog.kind', 'pg', { name: { en: 'PostgreSQL' }, category: 'data', behavior: 'primary_replica' }, (x) => lt(x, 'ru'))).toEqual({ title: 'PostgreSQL', subtitle: 'data · primary_replica' })
    expect(itemTitle('template', 't', { name: 'Plain', description: { ru: 'Описание' } }, (x) => lt(x, 'ru')).subtitle).toBe('Описание')
    expect(itemTitle('prompt', 'chat', { title: { en: 'Chat' }, role: 'fast', system: 'abcd' }, (x) => lt(x, 'en'))).toEqual({ title: 'Chat', subtitle: 'fast · 4 chars' })
  })

  it('falls back to the id', () => {
    expect(itemTitle('rule', 'R2', { check: 'x' }, lt).title).toBe('R2')
    expect(itemTitle('rule', 'R3', 'garbage', lt).title).toBe('R3')
    expect(itemTitle('engine', 'engine', {}, lt, 'Engine settings').title).toBe('Engine settings')
  })
})

describe('placement items', () => {
  beforeAll(() => setLang('en'))
  const pc = realPlacement()

  it('titles every shipped pattern and stack', () => {
    for (const p of pc.patterns) {
      const t = itemTitle('placement.pattern', p.id, p, (x) => lt(x, 'en'))
      expect(t.title, p.id).not.toBe(p.id)
      expect(t.subtitle, p.id).toContain(String(p.stack))
    }
    const single = pc.patterns.find((p) => p.minSites === 1 && p.maxSites === 1)!
    expect(itemTitle('placement.pattern', single.id, single, (x) => lt(x, 'en')).subtitle).toMatch(/^1 site · /)
    const many = pc.patterns.find((p) => typeof p.maxSites === 'number' && (p.maxSites as number) > 2)!
    expect(itemTitle('placement.pattern', many.id, many, (x) => lt(x, 'en')).subtitle).toMatch(new RegExp(`^${many.minSites}–${many.maxSites} sites`))
    const stack = pc.stacks[0]
    expect(itemTitle('placement.stack', stack.id, stack, (x) => lt(x, 'en')).subtitle).toBe(`${stack.layers.length} layers`)
    expect(itemTitle('placement.settings', 'placement', pc.settings, lt, 'Generator').title).toBe('Generator')
  })

  it('shows layer keys in a readable order without changing the content', () => {
    const stack = pc.stacks[0]
    const scrambled = { ...stack, layers: stack.layers.map((l) => Object.fromEntries(Object.entries(l).reverse())) }
    const o = orderedForEditor('placement.stack', scrambled) as { layers: Record<string, unknown>[] }
    expect(o).toEqual(scrambled)
    for (const l of o.layers) {
      const keys = Object.keys(l)
      expect(keys.indexOf('id'), String(l.id)).toBe(0)
      if ('kind' in l && 'scope' in l) expect(keys.indexOf('kind')).toBeLessThan(keys.indexOf('scope'))
    }
  })

  it('starts a new pattern with a stack that exists and the settings with the ones shipped', () => {
    const sk = skeleton('placement.pattern', 'p', { stacks: pc.stacks.map((s) => s.id) }) as Record<string, unknown>
    expect(pc.stacks.map((s) => s.id)).toContain(sk.stack)
    expect(sk.id).toBe('p')
    expect(Object.keys(orderedForEditor('placement.settings', pc.settings) as object).slice(0, 3)).toEqual(['defaultPattern', 'intraSite', 'intraGeo'])
  })
})

describe('search', () => {
  it('matches id, title and raw content', () => {
    const data = { check: 'min_instances', severity: 'risk' }
    expect(itemMatches('HA-LB-01', 'Balancers', data, 'ha-lb')).toBe(true)
    expect(itemMatches('HA-LB-01', 'Balancers', data, 'balanc')).toBe(true)
    expect(itemMatches('HA-LB-01', 'Balancers', data, 'min_inst')).toBe(true)
    expect(itemMatches('HA-LB-01', 'Balancers', data, 'zzz')).toBe(false)
    expect(itemMatches('a', 'b', data, '')).toBe(true)
  })
})

describe('ids and creation', () => {
  it('validates ids like the server', () => {
    expect(validItemId('my-rule_1.v2')).toBe(true)
    expect(validItemId('')).toBe(false)
    expect(validItemId('has space')).toBe(false)
    expect(validItemId('x'.repeat(81))).toBe(false)
    expect(validItemId('кириллица')).toBe(false)
  })

  it('forces data.id to the chosen id (except single documents)', () => {
    expect(withId('rule', 'new', { id: 'old', a: 1 })).toEqual({ id: 'new', a: 1 })
    expect(withId('rule', 'new', { a: 1 })).toEqual({ a: 1, id: 'new' })
    expect(withId('engine', 'engine', { a: 1 })).toEqual({ a: 1 })
  })

  it('provides a skeleton for every creatable kind', () => {
    for (const k of CONFIG_KINDS.filter((x) => !x.single)) {
      const s = skeleton(k.kind, 'x', { categories: ['ha'] }) as Record<string, unknown>
      expect(s.id, k.kind).toBe('x')
    }
    expect((skeleton('rule', 'x', { categories: ['dr'] }) as Record<string, unknown>).category).toBe('dr')
  })
})

describe('key order', () => {
  it('shows keys in the order of the engine types without changing the content', () => {
    const scrambled = { id: 'R', ok: {}, check: 'spof', title: {}, params: {}, weight: 1, enabled: true, message: {}, category: 'ha', severity: 'risk', custom: 1 }
    const o = orderedForEditor('rule', scrambled) as Record<string, unknown>
    expect(Object.keys(o)).toEqual(['id', 'category', 'check', 'enabled', 'severity', 'weight', 'params', 'title', 'message', 'ok', 'custom'])
    expect(o).toEqual(scrambled)
  })

  it('orders property definitions inside props and common properties', () => {
    const p = { label: 'l', type: 'number', key: 'k', min: 1 }
    const kind = orderedForEditor('catalog.kind', { props: [p], id: 'x' }) as { props: Record<string, unknown>[] }
    expect(Object.keys(kind.props[0])).toEqual(['key', 'type', 'label', 'min'])
    const common = orderedForEditor('catalog.common_props', [p]) as Record<string, unknown>[]
    expect(Object.keys(common[0])).toEqual(['key', 'type', 'label', 'min'])
  })

  it('leaves non-objects alone', () => {
    expect(orderedForEditor('rule', 5)).toBe(5)
    expect(orderedForEditor('engine', null)).toBeNull()
  })
})

describe('prompt placeholders', () => {
  it('knows what the server substitutes', () => {
    expect(promptVars('repair')).toEqual(['errors'])
    expect(promptVars('refine')).toEqual(['findings'])
    expect(promptVars('patch')).toContain('architecture')
    expect(promptVars('chat')).toContain('report')
    expect(promptVars('generate')).not.toContain('architecture')
    expect(promptVars('custom')).toContain('language')
  })

  it('finds placeholders as written and flags those the server will not replace', () => {
    const text = 'a {{errors}} b {{ errors }} c {{unknown}} {single} {{}}'
    expect(placeholdersIn(text)).toEqual(['errors', ' errors ', 'unknown'])
    expect(unknownPlaceholders(text, ['errors'])).toEqual([' errors ', 'unknown'])
    expect(unknownPlaceholders('{{language}}', promptVars('generate'))).toEqual([])
  })
})
