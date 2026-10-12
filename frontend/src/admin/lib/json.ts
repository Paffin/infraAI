// JSON helpers for the admin editors: validation with a precise error position (the messages of
// JSON.parse differ between browsers and often lack a position), formatting and key anchors.

export type JsonErrorCode =
  | 'unexpected_token' | 'unexpected_end' | 'expected_comma' | 'expected_colon' | 'expected_key'
  | 'trailing_comma' | 'trailing_data' | 'bad_number' | 'bad_escape' | 'unterminated_string'
  | 'control_char' | 'too_deep' | 'syntax'

export interface JsonOk { ok: true; value: unknown }
export interface JsonErr {
  ok: false
  code: JsonErrorCode
  params?: Record<string, string>
  /** 0-based character offset of the problem (when known) */
  offset?: number
  /** 1-based */
  line?: number
  column?: number
  /** the raw message of the engine's JSON.parse, for tooltips */
  native?: string
}
export type JsonResult = JsonOk | JsonErr

interface ScanError { code: JsonErrorCode; params?: Record<string, string>; offset: number }
export type PathNode = { path: (string | number)[]; offset: number }

const WS = new Set([' ', '\t', '\n', '\r'])
const MAX_DEPTH = 400

/**
 * Minimal strict JSON scanner. It decides nothing about validity (JSON.parse does) but finds *where*
 * and *why* the first syntax error is. `onNode` is called for every value start at depth <= `nodeDepth`.
 */
export function scanJson(text: string, nodeDepth = 0, onNode?: (n: PathNode) => void): ScanError | null {
  let i = 0
  const n = text.length
  const skip = () => { while (i < n && WS.has(text[i])) i++ }
  const fail = (code: JsonErrorCode, offset = i, params?: Record<string, string>): ScanError => ({ code, offset, params })
  const tok = (): ScanError => (i >= n ? fail('unexpected_end') : fail('unexpected_token', i, { char: text[i] }))

  function str(): ScanError | null {
    const start = i
    i++
    while (i < n) {
      const c = text[i]
      if (c === '"') { i++; return null }
      if (c === '\\') {
        const e = text[i + 1]
        if (e === undefined) return fail('unterminated_string', start)
        if (e === 'u') {
          if (!/^[0-9a-fA-F]{4}$/.test(text.slice(i + 2, i + 6))) return fail('bad_escape', i)
          i += 6
          continue
        }
        if (!'"\\/bfnrt'.includes(e)) return fail('bad_escape', i)
        i += 2
        continue
      }
      if (c < ' ') return fail('control_char', i)
      i++
    }
    return fail('unterminated_string', start)
  }

  function num(): ScanError | null {
    const m = /^-?(0|[1-9]\d*)(\.\d+)?([eE][+-]?\d+)?/.exec(text.slice(i, i + 400))
    if (!m) return fail('bad_number')
    i += m[0].length
    return null
  }

  function value(path: (string | number)[], depth: number): ScanError | null {
    if (depth > MAX_DEPTH) return fail('too_deep')
    skip()
    if (i >= n) return fail('unexpected_end')
    if (onNode && path.length <= nodeDepth) onNode({ path, offset: i })
    const c = text[i]
    if (c === '{') {
      i++
      skip()
      if (text[i] === '}') { i++; return null }
      for (;;) {
        skip()
        if (i >= n) return fail('unexpected_end')
        if (text[i] !== '"') return text[i] === '}' ? fail('trailing_comma') : fail('expected_key')
        const keyStart = i
        const e = str()
        if (e) return e
        const key = safeKey(text.slice(keyStart, i))
        skip()
        if (i >= n) return fail('unexpected_end')
        if (text[i] !== ':') return fail('expected_colon')
        i++
        const v = value([...path, key], depth + 1)
        if (v) return v
        skip()
        if (i >= n) return fail('unexpected_end')
        if (text[i] === ',') { i++; continue }
        if (text[i] === '}') { i++; return null }
        return fail('expected_comma')
      }
    }
    if (c === '[') {
      i++
      skip()
      if (text[i] === ']') { i++; return null }
      let idx = 0
      for (;;) {
        skip()
        if (text[i] === ']') return fail('trailing_comma')
        const v = value([...path, idx++], depth + 1)
        if (v) return v
        skip()
        if (i >= n) return fail('unexpected_end')
        if (text[i] === ',') { i++; continue }
        if (text[i] === ']') { i++; return null }
        return fail('expected_comma')
      }
    }
    if (c === '"') return str()
    if (c === '-' || (c >= '0' && c <= '9')) return num()
    for (const lit of ['true', 'false', 'null']) {
      if (text.startsWith(lit, i)) { i += lit.length; return null }
    }
    return tok()
  }

  const err = value([], 0)
  if (err) return err
  skip()
  if (i < n) return fail('trailing_data')
  return null
}

function safeKey(raw: string): string {
  try { return String(JSON.parse(raw)) } catch { return raw.slice(1, -1) }
}

export function offsetToLineCol(text: string, offset: number): { line: number; column: number } {
  const o = Math.max(0, Math.min(offset, text.length))
  let line = 1
  let last = -1
  for (let i = 0; i < o; i++) {
    if (text.charCodeAt(i) === 10) { line++; last = i }
  }
  return { line, column: o - last }
}

export function lineColToOffset(text: string, line: number, column = 1): number {
  let off = 0
  for (let l = 1; l < line; l++) {
    const nl = text.indexOf('\n', off)
    if (nl < 0) return text.length
    off = nl + 1
  }
  return Math.min(text.length, off + Math.max(0, column - 1))
}

/** Parse `text`; on failure return a code, parameters and the position of the first problem. */
export function parseJson(text: string): JsonResult {
  try {
    return { ok: true, value: JSON.parse(text) }
  } catch (e) {
    const native = e instanceof Error ? e.message : String(e)
    const found = scanJson(text)
    if (found) {
      const { line, column } = offsetToLineCol(text, found.offset)
      return { ok: false, code: found.code, params: found.params, offset: found.offset, line, column, native }
    }
    // The scanner and the engine disagree (should not happen): fall back to the engine's position.
    const pos = /position (\d+)/.exec(native)
    if (pos) {
      const offset = Number(pos[1])
      const { line, column } = offsetToLineCol(text, offset)
      return { ok: false, code: 'syntax', offset, line, column, native }
    }
    return { ok: false, code: 'syntax', native }
  }
}

/** Pretty-print valid JSON text; returns null for invalid input. */
export function formatJson(text: string, indent = 2): string | null {
  const r = parseJson(text)
  return r.ok ? JSON.stringify(r.value, null, indent) : null
}

export function pretty(value: unknown): string {
  return JSON.stringify(value ?? null, null, 2)
}

/** Stable JSON (sorted keys) for deep equality of plain data. */
export function stableStringify(v: unknown): string {
  if (Array.isArray(v)) return '[' + v.map(stableStringify).join(',') + ']'
  if (v && typeof v === 'object') {
    const o = v as Record<string, unknown>
    return '{' + Object.keys(o).sort().map((k) => JSON.stringify(k) + ':' + stableStringify(o[k])).join(',') + '}'
  }
  return JSON.stringify(v === undefined ? null : v)
}

export function deepEqual(a: unknown, b: unknown): boolean {
  return stableStringify(a) === stableStringify(b)
}

/** Offsets of values at depth <= `depth` keyed by their path joined with `/` (for "jump to section" anchors). */
export function anchorOffsets(text: string, depth = 1): Map<string, number> {
  const out = new Map<string, number>()
  scanJson(text, depth, (n) => { if (n.path.length) out.set(n.path.join('/'), n.offset) })
  return out
}

export function isPlainObject(v: unknown): v is Record<string, unknown> {
  return !!v && typeof v === 'object' && !Array.isArray(v)
}
