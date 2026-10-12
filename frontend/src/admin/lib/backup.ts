// Backup / migration payloads: file naming, parsing of an exported file and a summary for the preview.

import { isPlainObject, parseJson } from './json'

export const EXPORT_FORMAT = 'infra-architect-config/1'

/** `infra-architect-config-2026-10-12T20-34-33.json` (local time, safe for every file system). */
export function exportFileName(now: Date = new Date()): string {
  const p = (n: number) => String(n).padStart(2, '0')
  const stamp = `${now.getFullYear()}-${p(now.getMonth() + 1)}-${p(now.getDate())}T${p(now.getHours())}-${p(now.getMinutes())}-${p(now.getSeconds())}`
  return `infra-architect-config-${stamp}.json`
}

export interface ImportPayload { settings: Record<string, unknown>; items: unknown[] }
export interface ImportSummary { format: string | null; settings: number; items: number; byKind: [string, number][]; formatOk: boolean }
export type ParsedImport = { ok: true; payload: ImportPayload; summary: ImportSummary } | { ok: false; reason: 'json' | 'shape' | 'empty' }

/** Read the text of an export file (or hand-written JSON) into the body of `POST /admin/config-import`. */
export function parseImport(text: string): ParsedImport {
  const r = parseJson(text)
  if (!r.ok) return { ok: false, reason: 'json' }
  const v = r.value
  if (!isPlainObject(v)) return { ok: false, reason: 'shape' }
  const settings = v.settings === undefined ? {} : v.settings
  const items = v.items === undefined ? [] : v.items
  if (!isPlainObject(settings) || !Array.isArray(items)) return { ok: false, reason: 'shape' }
  if (!Object.keys(settings).length && !items.length) return { ok: false, reason: 'empty' }
  const kinds = new Map<string, number>()
  for (const it of items) {
    const kind = isPlainObject(it) && typeof it.kind === 'string' ? it.kind : '?'
    kinds.set(kind, (kinds.get(kind) ?? 0) + 1)
  }
  const format = typeof v.format === 'string' ? v.format : null
  return {
    ok: true,
    payload: { settings, items },
    summary: { format, settings: Object.keys(settings).length, items: items.length, byKind: [...kinds.entries()].sort(([a], [b]) => a.localeCompare(b)), formatOk: format === EXPORT_FORMAT },
  }
}
