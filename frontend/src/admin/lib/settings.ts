// Pure logic of the settings editor: edit values, validation (mirrors `settings::validate` of the
// server), dirty tracking, grouping and the payload of `PUT /admin/settings`.

import type { SettingDef } from '@/api/types'
import { deepEqual, parseJson, pretty, type JsonErr } from './json'

export interface SecretDraft { text: string; clear: boolean }
/** What a form control edits: numbers and JSON are edited as text, lists as arrays, secrets as {text, clear}. */
export type EditValue = string | boolean | string[] | SecretDraft

export type SettingErrorCode = 'string' | 'number' | 'min' | 'max' | 'enum' | 'boolean' | 'list' | 'json' | 'json_object'
export interface SettingError { code: SettingErrorCode; params?: Record<string, unknown>; json?: JsonErr }
export type Validated = { ok: true; value: unknown } | { ok: false; error: SettingError }

export function isSecret(def: SettingDef): boolean {
  return def.type === 'secret'
}

export function isSecretDraft(v: EditValue | undefined): v is SecretDraft {
  return !!v && typeof v === 'object' && !Array.isArray(v)
}

/** The value a freshly loaded setting is edited from. */
export function toEdit(def: SettingDef): EditValue {
  switch (def.type) {
    case 'secret': return { text: '', clear: false }
    case 'boolean': return def.value === true
    case 'number': return def.value == null ? '' : String(def.value)
    case 'string_list': return Array.isArray(def.value) ? def.value.filter((x): x is string => typeof x === 'string') : []
    case 'json': return pretty(def.value ?? def.default)
    default: return def.value == null ? '' : String(def.value)
  }
}

/** The edit value that corresponds to the built-in default. */
export function defaultEdit(def: SettingDef): EditValue {
  return toEdit({ ...def, value: def.default })
}

/** Normalise and validate an edit value the way the server will. For secrets `value: undefined` means "unchanged". */
export function fromEdit(def: SettingDef, edit: EditValue): Validated {
  switch (def.type) {
    case 'string':
    case 'model':
      return typeof edit === 'string' ? { ok: true, value: edit.trim() } : bad('string')
    case 'enum': {
      const s = typeof edit === 'string' ? edit : ''
      return (def.options ?? []).includes(s) ? { ok: true, value: s } : bad('enum', { options: (def.options ?? []).map((o) => o || '—').join(', ') })
    }
    case 'number': {
      const s = typeof edit === 'string' ? edit.trim() : ''
      const n = s === '' ? NaN : Number(s)
      if (!Number.isFinite(n)) return bad('number')
      if (def.min != null && n < def.min) return bad('min', { min: def.min })
      if (def.max != null && n > def.max) return bad('max', { max: def.max })
      return { ok: true, value: n }
    }
    case 'boolean':
      return typeof edit === 'boolean' ? { ok: true, value: edit } : bad('boolean')
    case 'string_list':
      return Array.isArray(edit) ? { ok: true, value: normalizeList(edit) } : bad('list')
    case 'json': {
      if (typeof edit !== 'string') return bad('json')
      const p = parseJson(edit)
      if (!p.ok) return { ok: false, error: { code: 'json', json: p } }
      return p.value !== null && typeof p.value === 'object' ? { ok: true, value: p.value } : bad('json_object')
    }
    case 'secret': {
      if (!isSecretDraft(edit)) return { ok: true, value: undefined }
      if (edit.clear) return { ok: true, value: '' }
      const s = edit.text.trim()
      return { ok: true, value: s === '' ? undefined : s }
    }
    default:
      return { ok: true, value: edit }
  }
}

function bad(code: SettingErrorCode, params?: Record<string, unknown>): Validated {
  return { ok: false, error: { code, params } }
}

export function normalizeList(list: string[]): string[] {
  return list.map((s) => s.trim()).filter((s) => s !== '')
}

/** "a, b\nc" -> ['a','b','c'] (pasted lists). */
export function splitList(text: string): string[] {
  return normalizeList(text.split(/[\n,;]+/))
}

/** Does the edit value differ from what the server currently stores? */
export function isDirty(def: SettingDef, edit: EditValue | undefined): boolean {
  if (edit === undefined) return false
  if (isSecret(def)) return isSecretDraft(edit) && (edit.clear || edit.text.trim() !== '')
  const r = fromEdit(def, edit)
  if (!r.ok) return true
  return !deepEqual(r.value, def.value)
}

/** Differs from the built-in default (what "reset to default" would change). */
export function differsFromDefault(def: SettingDef, edit: EditValue | undefined): boolean {
  if (isSecret(def)) return !!def.secretSet || (isSecretDraft(edit) && edit.text.trim() !== '')
  if (edit === undefined) return !!def.modified
  const r = fromEdit(def, edit)
  if (!r.ok) return true
  return !deepEqual(r.value, def.default)
}

export interface Changes {
  changes: Record<string, unknown>
  errors: Record<string, SettingError>
}

/** Build the body of `PUT /admin/settings`: only changed keys, validated; `errors` are keyed by setting key. */
export function buildChanges(defs: SettingDef[], drafts: Record<string, EditValue>): Changes {
  const changes: Record<string, unknown> = {}
  const errors: Record<string, SettingError> = {}
  for (const def of defs) {
    const edit = drafts[def.key]
    if (!isDirty(def, edit)) continue
    const r = fromEdit(def, edit as EditValue)
    if (!r.ok) { errors[def.key] = r.error; continue }
    if (r.value !== undefined) changes[def.key] = r.value
  }
  return { changes, errors }
}

export function dirtyKeys(defs: SettingDef[], drafts: Record<string, EditValue>): string[] {
  return defs.filter((d) => isDirty(d, drafts[d.key])).map((d) => d.key)
}

export interface SettingGroup { group: string; items: SettingDef[]; modified: number }

/** Groups in the order of first appearance in the registry; `modified` counts values stored in the database. */
export function groupSettings(defs: SettingDef[]): SettingGroup[] {
  const map = new Map<string, SettingGroup>()
  for (const d of defs) {
    let g = map.get(d.group)
    if (!g) { g = { group: d.group, items: [], modified: 0 }; map.set(d.group, g) }
    g.items.push(d)
    if (isSecret(d) ? d.secretSource === 'database' : d.modified) g.modified++
  }
  return [...map.values()]
}

/**
 * The server reports only the first failing key as one message: "<key>: <reason>" (or
 * "unknown setting '<key>'"). Extract the key so the error can be shown next to the control.
 */
export function parseServerSettingError(message: string, keys: Iterable<string>): { key: string | null; message: string } {
  const known = new Set(keys)
  const m = /^([A-Za-z0-9_.]+): ([\s\S]+)$/.exec(message)
  if (m && known.has(m[1])) return { key: m[1], message: m[2] }
  const u = /unknown setting '([^']+)'/.exec(message)
  if (u && known.has(u[1])) return { key: u[1], message }
  return { key: null, message }
}

export interface DefaultLabels { yes: string; no: string; empty: string }

/** Short human-readable rendering of a setting's built-in default. */
export function formatDefault(def: SettingDef, labels: DefaultLabels): string {
  const v = def.default
  switch (def.type) {
    case 'boolean': return v ? labels.yes : labels.no
    case 'secret': return labels.empty
    case 'string_list': return Array.isArray(v) && v.length ? v.join(', ') : labels.empty
    case 'json': {
      const s = JSON.stringify(v)
      return !s || s === '{}' || s === '[]' ? labels.empty : s.length > 80 ? s.slice(0, 77) + '…' : s
    }
    default: return v === '' || v == null ? labels.empty : String(v)
  }
}

/** Case-insensitive match on key and the (already localised) label/help. */
export function settingMatches(def: SettingDef, query: string, label: string, help: string): boolean {
  const q = query.trim().toLowerCase()
  if (!q) return true
  return def.key.toLowerCase().includes(q) || label.toLowerCase().includes(q) || help.toLowerCase().includes(q)
}
