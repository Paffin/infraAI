// Config item kinds: routing, titles, skeletons for new items and prompt placeholders.

import type { LText } from '@/api/types'
import { lt as defaultLt, t } from '@/i18n'
import { isPlainObject } from './json'

export interface KindMeta {
  kind: string
  /** value of the `:section` route parameter */
  section: string
  /** value of the `:sub` route parameter (catalog kinds only) */
  sub?: string
  /** a single document that cannot be created or deleted (`catalog.common_props`, `engine`) */
  single: boolean
}

export const CONFIG_KINDS: KindMeta[] = [
  { kind: 'catalog.kind', section: 'catalog', sub: 'kind', single: false },
  { kind: 'catalog.edge_type', section: 'catalog', sub: 'edge_type', single: false },
  { kind: 'catalog.zone_type', section: 'catalog', sub: 'zone_type', single: false },
  { kind: 'catalog.category', section: 'catalog', sub: 'category', single: false },
  { kind: 'catalog.common_props', section: 'catalog', sub: 'common_props', single: true },
  { kind: 'rule', section: 'rules', single: false },
  { kind: 'template', section: 'templates', single: false },
  { kind: 'iac', section: 'iac', single: false },
  { kind: 'prompt', section: 'prompts', single: false },
  { kind: 'engine', section: 'engine', single: true },
]

export function kindMeta(kind: string): KindMeta | undefined {
  return CONFIG_KINDS.find((k) => k.kind === kind)
}

export function kindFromRoute(section: string | undefined, sub: string | undefined): KindMeta | undefined {
  if (!section) return undefined
  if (section === 'catalog') return CONFIG_KINDS.find((k) => k.section === 'catalog' && k.sub === (sub || 'kind'))
  return CONFIG_KINDS.find((k) => k.section === section)
}

/** Router location of a kind's page, optionally opening one item. */
export function kindPath(kind: string, itemId?: string): { path: string; query?: Record<string, string> } {
  const m = kindMeta(kind)
  const path = m ? `/admin/${m.section}${m.sub ? '/' + m.sub : ''}` : '/admin/settings'
  return itemId ? { path, query: { item: itemId } } : { path }
}

export const SECTION_IDS = ['settings', 'llm', 'catalog', 'rules', 'templates', 'iac', 'prompts', 'engine', 'backup', 'users', 'audit', 'system'] as const
export const CONFIG_SECTIONS = ['catalog', 'rules', 'templates', 'iac', 'prompts', 'engine'] as const
export type SectionId = typeof SECTION_IDS[number]

export function normalizeSection(s: unknown): SectionId {
  return (SECTION_IDS as readonly string[]).includes(String(s)) ? (s as SectionId) : 'settings'
}

// ---------------------------------------------------------------------------- titles

export interface ItemTitle { title: string; subtitle: string }

function pick(data: Record<string, unknown>, ...keys: string[]): LText | undefined {
  for (const k of keys) {
    const v = data[k]
    if (typeof v === 'string' || isPlainObject(v)) return v as LText
  }
  return undefined
}

/** Human title of a config item, derived from its data (names/titles are LText). */
export function itemTitle(kind: string, id: string, data: unknown, lt: (x: LText | null | undefined) => string = defaultLt, fallbackTitle = ''): ItemTitle {
  if (kind === 'catalog.common_props' || kind === 'engine') return { title: fallbackTitle || id, subtitle: '' }
  if (!isPlainObject(data)) return { title: id, subtitle: '' }
  const title = lt(pick(data, 'title', 'name')) || id
  const s = (k: string) => (typeof data[k] === 'string' ? (data[k] as string) : '')
  let subtitle = ''
  switch (kind) {
    case 'rule': subtitle = [s('category'), s('check')].filter(Boolean).join(' · '); break
    case 'catalog.kind': subtitle = [s('category'), s('behavior')].filter(Boolean).join(' · '); break
    case 'catalog.edge_type': subtitle = s('semantics'); break
    case 'catalog.zone_type': subtitle = s('level'); break
    case 'prompt': subtitle = [s('role'), s('system') ? t('admin.prompt.charsShort', { n: s('system').length }) : ''].filter(Boolean).join(' · '); break
    case 'template': subtitle = lt(pick(data, 'description')); break
    case 'iac': subtitle = [s('target'), s('granularity'), data.enabled === false ? t('admin.iac.disabled') : ''].filter(Boolean).join(' · '); break
    default: break
  }
  return { title, subtitle }
}

/** Search over id, title and the raw JSON of an item (so values like a `check` type are found too). */
export function itemMatches(id: string, title: string, data: unknown, query: string): boolean {
  const q = query.trim().toLowerCase()
  if (!q) return true
  if (id.toLowerCase().includes(q) || title.toLowerCase().includes(q)) return true
  try {
    return JSON.stringify(data).toLowerCase().includes(q)
  } catch {
    return false
  }
}

const ID_RE = /^[A-Za-z0-9_.-]{1,80}$/
export function validItemId(id: string): boolean {
  return ID_RE.test(id)
}

// ---------------------------------------------------------------------------- prompts

/** `{{placeholders}}` the server substitutes, per prompt id (see llm/pipeline.rs `common_vars`). */
const COMMON_VARS = ['language', 'catalog', 'edge_types', 'zone_types', 'templates', 'rules', 'requirements_schema', 'max_ops']
export const PROMPT_VARS: Record<string, string[]> = {
  generate: COMMON_VARS,
  patch: [...COMMON_VARS, 'architecture', 'findings'],
  chat: [...COMMON_VARS, 'architecture', 'report'],
  repair: ['errors'],
  refine: ['findings'],
}

/** Custom prompts are not used by the pipeline, but the common variables are what a copy of `patch` may use. */
export function promptVars(id: string): string[] {
  return PROMPT_VARS[id] ?? COMMON_VARS
}

/** `{{name}}` occurrences as written (the server replaces only the exact `{{name}}` spelling). */
export function placeholdersIn(text: string): string[] {
  const out = new Set<string>()
  for (const m of text.matchAll(/\{\{([^{}\n]+)\}\}/g)) out.add(m[1])
  return [...out]
}

/** Placeholders written in the text that the server will leave as they are. */
export function unknownPlaceholders(text: string, allowed: string[]): string[] {
  return placeholdersIn(text).filter((p) => !allowed.includes(p))
}

// ---------------------------------------------------------------------------- key order

// The database stores items as jsonb, which returns object keys sorted by length. For editing, show them
// in the order of the engine types (the order the defaults are written in).
const KEY_ORDER: Record<string, string[]> = {
  rule: ['id', 'category', 'check', 'enabled', 'severity', 'weight', 'params', 'title', 'message', 'noDataMessage', 'requirementMessage', 'ok', 'rationale', 'remediation', 'fixLabels', 'references'],
  'catalog.kind': ['id', 'category', 'name', 'description', 'abbr', 'color', 'behavior', 'traits', 'roles', 'props', 'subtitle', 'capacity', 'cost', 'runbook', 'hidden'],
  'catalog.edge_type': ['id', 'name', 'semantics', 'style', 'directed', 'defaultProps', 'props', 'layer'],
  'catalog.zone_type': ['id', 'name', 'level', 'color', 'props'],
  'catalog.category': ['id', 'name', 'order'],
  template: ['id', 'name', 'description', 'badge', 'enabled', 'order', 'architecture'],
  iac: ['id', 'title', 'enabled', 'target', 'reason', 'granularity', 'requires', 'unsupported', 'needs', 'vars', 'objects', 'releases', 'secrets', 'notes'],
  prompt: ['id', 'title', 'role', 'system'],
  engine: ['assumptions', 'scoring', 'scenarios', 'cost', 'export'],
}
const PROP_ORDER = ['key', 'type', 'label', 'unit', 'min', 'max', 'options', 'default', 'group', 'help', 'refBehavior', 'advanced']
const ARCH_ORDER = ['schemaVersion', 'meta', 'requirements', 'zones', 'nodes', 'edges']

function reorder(o: Record<string, unknown>, order: string[]): Record<string, unknown> {
  const out: Record<string, unknown> = {}
  for (const k of order) if (k in o) out[k] = o[k]
  for (const k of Object.keys(o)) if (!(k in out)) out[k] = o[k]
  return out
}

function orderProps(v: unknown): unknown {
  return Array.isArray(v) ? v.map((p) => (isPlainObject(p) ? reorder(p, PROP_ORDER) : p)) : v
}

/** The same data with keys in a readable order (presentation only; the content is unchanged). */
export function orderedForEditor(kind: string, data: unknown): unknown {
  if (kind === 'catalog.common_props') return orderProps(data)
  if (!isPlainObject(data)) return data
  const out = reorder(data, KEY_ORDER[kind] ?? ['id'])
  if ('props' in out) out.props = orderProps(out.props)
  if (kind === 'template' && isPlainObject(out.architecture)) out.architecture = reorder(out.architecture, ARCH_ORDER)
  return out
}

// ---------------------------------------------------------------------------- skeletons

function cssVar(name: string): string {
  try {
    return getComputedStyle(document.documentElement).getPropertyValue(name).trim()
  } catch {
    return ''
  }
}

const empty = (): Record<string, string> => ({ ru: '', en: '' })

/** A minimal valid document for a new custom item (the id is injected on creation). */
export function skeleton(kind: string, id: string, ctx: { categories?: string[] } = {}): unknown {
  const color = cssVar('--accent')
  switch (kind) {
    case 'catalog.category': return { id, name: empty(), order: 100 }
    case 'catalog.zone_type': return { id, name: empty(), level: 'site', color, props: [] }
    case 'catalog.edge_type': return { id, name: empty(), semantics: 'dependency', style: { color, width: 1.6 }, directed: true, defaultProps: {}, props: [] }
    case 'catalog.kind': return { id, category: ctx.categories?.[0] ?? '', name: empty(), description: empty(), abbr: 'NEW', color, behavior: 'stateless', traits: [], roles: [], props: [] }
    case 'rule': return { id, category: ctx.categories?.[0] ?? 'ha', check: 'spof', enabled: true, severity: 'risk', params: {}, title: empty(), message: empty(), ok: empty(), fixLabels: {}, references: [] }
    case 'template': return { id, name: empty(), description: empty(), badge: '', enabled: true, order: 100, architecture: { schemaVersion: 1, meta: {}, requirements: {}, zones: [], nodes: [], edges: [] } }
    case 'prompt': return { id, title: empty(), system: '', role: 'planner' }
    case 'iac': return { id, title: empty(), enabled: true, target: 'kubernetes', granularity: 'unit', requires: [], objects: [] }
    default: return { id }
  }
}

/** `data.id` must equal the item id for every kind except the single documents. */
export function withId(kind: string, id: string, data: unknown): unknown {
  const m = kindMeta(kind)
  if (m?.single || !isPlainObject(data)) return data
  return { ...data, id }
}
