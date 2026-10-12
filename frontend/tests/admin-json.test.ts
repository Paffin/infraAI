import { describe, expect, it } from 'vitest'
import { anchorOffsets, deepEqual, formatJson, lineColToOffset, offsetToLineCol, parseJson, scanJson, stableStringify } from '../src/admin/lib/json'

describe('parseJson', () => {
  it('accepts valid documents', () => {
    const r = parseJson('{"a": [1, 2, {"b": null}], "c": "x\\n"}')
    expect(r.ok).toBe(true)
    if (r.ok) expect(r.value).toEqual({ a: [1, 2, { b: null }], c: 'x\n' })
  })

  it('locates a missing comma on the next line', () => {
    const text = '{\n  "a": 1\n  "b": 2\n}'
    const r = parseJson(text)
    expect(r.ok).toBe(false)
    if (!r.ok) {
      expect(r.code).toBe('expected_comma')
      expect(r.line).toBe(3)
      expect(r.column).toBe(3)
    }
  })

  it('reports each kind of syntax error with a code', () => {
    const code = (t: string) => { const r = parseJson(t); return r.ok ? 'ok' : r.code }
    expect(code('')).toBe('unexpected_end')
    expect(code('{"a": }')).toBe('unexpected_token')
    expect(code('{"a" 1}')).toBe('expected_colon')
    expect(code('{a: 1}')).toBe('expected_key')
    expect(code('{"a": 1,}')).toBe('trailing_comma')
    expect(code('[1, 2,]')).toBe('trailing_comma')
    expect(code('[1 2]')).toBe('expected_comma')
    expect(code('{"a": 1} x')).toBe('trailing_data')
    expect(code('{"a": 01}')).toBe('expected_comma')
    expect(code('"abc')).toBe('unterminated_string')
    expect(code('"a\\qb"')).toBe('bad_escape')
    expect(code('"a\nb"')).toBe('control_char')
    expect(code('{"a": tru}')).toBe('unexpected_token')
    expect(code('{"a": -}')).toBe('bad_number')
  })

  it('includes the offending character', () => {
    const r = parseJson('{"a": }')
    expect(r.ok).toBe(false)
    if (!r.ok) expect(r.params).toEqual({ char: '}' })
  })

  it('agrees with JSON.parse on a set of documents', () => {
    const docs = ['{}', '[]', '1', '"s"', 'null', 'true', '[1,2', '{"a":1}}', '{"a":[1,{"b":2}]}', '[1.5e3, -0.1, 0]', '{"a":"\\u00e9"}', '{"a":"\\u00e"}', '[01]', '[1.]', '[.5]', "{'a':1}", '{"a":1,"a":2}', ' \n\t[ ] ']
    for (const d of docs) {
      let valid = true
      try { JSON.parse(d) } catch { valid = false }
      expect(scanJson(d) === null, d).toBe(valid)
    }
  })

  it('does not blow the stack on deep nesting', () => {
    const r = parseJson('['.repeat(5000))
    expect(r.ok).toBe(false)
  })
})

describe('positions', () => {
  it('converts offsets and line/column both ways', () => {
    const text = 'ab\ncde\nf'
    expect(offsetToLineCol(text, 0)).toEqual({ line: 1, column: 1 })
    expect(offsetToLineCol(text, 4)).toEqual({ line: 2, column: 2 })
    expect(offsetToLineCol(text, 7)).toEqual({ line: 3, column: 1 })
    expect(lineColToOffset(text, 2, 2)).toBe(4)
    expect(lineColToOffset(text, 3, 1)).toBe(7)
    expect(lineColToOffset(text, 9, 1)).toBe(text.length)
  })
})

describe('formatJson / stable equality', () => {
  it('pretty-prints valid JSON and refuses invalid', () => {
    expect(formatJson('{"a":[1,2]}')).toBe('{\n  "a": [\n    1,\n    2\n  ]\n}')
    expect(formatJson('{"a":')).toBeNull()
  })

  it('compares data regardless of key order', () => {
    expect(deepEqual({ a: 1, b: { c: [1, 2] } }, { b: { c: [1, 2] }, a: 1 })).toBe(true)
    expect(deepEqual({ a: 1 }, { a: 2 })).toBe(false)
    expect(deepEqual([1, 2], [2, 1])).toBe(false)
    expect(stableStringify({ b: 1, a: undefined })).toBe('{"a":null,"b":1}')
  })
})

describe('anchorOffsets', () => {
  it('finds the line of every top-level section', () => {
    const text = JSON.stringify({ assumptions: { a: 1 }, scoring: { b: 2 }, cost: [1] }, null, 2)
    const a = anchorOffsets(text, 1)
    const lineOf = (k: string) => offsetToLineCol(text, a.get(k)!).line
    expect(lineOf('assumptions')).toBe(2)
    expect(lineOf('scoring')).toBe(5)
    expect(lineOf('cost')).toBe(8)
  })

  it('indexes array elements', () => {
    const text = JSON.stringify([{ key: 'x' }, { key: 'y' }], null, 2)
    const a = anchorOffsets(text, 1)
    expect(offsetToLineCol(text, a.get('1')!).line).toBe(5)
  })
})
