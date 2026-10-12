import { describe, expect, it } from 'vitest'
import type { SettingDef } from '../src/api/types'
import {
  buildChanges, defaultEdit, differsFromDefault, dirtyKeys, formatDefault, fromEdit, groupSettings, isDirty,
  parseServerSettingError, settingMatches, splitList, toEdit, type EditValue,
} from '../src/admin/lib/settings'

const def = (o: Partial<SettingDef> & Pick<SettingDef, 'key' | 'type'>): SettingDef => ({ group: 'general', default: null, label: o.key, ...o })

const num = def({ key: 'auth.ttl', type: 'number', default: 12, value: 12, min: 1, max: 720, group: 'auth' })
const flag = def({ key: 'llm.enabled', type: 'boolean', default: true, value: true, group: 'llm' })
const mode = def({ key: 'llm.mode', type: 'enum', default: 'a', value: 'a', options: ['', 'a', 'b'], group: 'llm' })
const text = def({ key: 'app.name', type: 'string', default: 'App', value: 'App' })
const list = def({ key: 'llm.fallbacks', type: 'string_list', default: [], value: ['x'], modified: true, group: 'llm' })
const json = def({ key: 'ui.theme', type: 'json', default: {}, value: { '--a': 'b' }, modified: true })
const secret = def({ key: 'llm.apiKey', type: 'secret', default: '', secretSet: false, secretSource: 'environment', group: 'llm' })
const model = def({ key: 'llm.model', type: 'model', default: 'm/a', value: 'm/b', modified: true, group: 'llm' })

describe('toEdit / fromEdit', () => {
  it('round-trips every type', () => {
    expect(toEdit(num)).toBe('12')
    expect(toEdit(flag)).toBe(true)
    expect(toEdit(list)).toEqual(['x'])
    expect(toEdit(json)).toBe('{\n  "--a": "b"\n}')
    expect(toEdit(secret)).toEqual({ text: '', clear: false })
    for (const d of [num, flag, mode, text, list, json, model]) {
      const r = fromEdit(d, toEdit(d))
      expect(r.ok && r.value, d.key).toEqual(d.value)
    }
  })

  it('validates numbers like the server', () => {
    expect(fromEdit(num, '')).toMatchObject({ ok: false, error: { code: 'number' } })
    expect(fromEdit(num, 'abc')).toMatchObject({ ok: false, error: { code: 'number' } })
    expect(fromEdit(num, '0')).toMatchObject({ ok: false, error: { code: 'min', params: { min: 1 } } })
    expect(fromEdit(num, '721')).toMatchObject({ ok: false, error: { code: 'max', params: { max: 720 } } })
    expect(fromEdit(num, ' 24 ')).toEqual({ ok: true, value: 24 })
    expect(fromEdit(num, '1.5')).toEqual({ ok: true, value: 1.5 })
  })

  it('validates enums, JSON and lists', () => {
    expect(fromEdit(mode, 'c')).toMatchObject({ ok: false, error: { code: 'enum' } })
    expect(fromEdit(mode, '')).toEqual({ ok: true, value: '' })
    expect(fromEdit(json, '{"a": }')).toMatchObject({ ok: false, error: { code: 'json' } })
    expect(fromEdit(json, '"s"')).toMatchObject({ ok: false, error: { code: 'json_object' } })
    expect(fromEdit(json, '[1]')).toEqual({ ok: true, value: [1] })
    expect(fromEdit(list, ['  a ', '', 'b'])).toEqual({ ok: true, value: ['a', 'b'] })
  })

  it('treats secrets as write-only', () => {
    expect(fromEdit(secret, { text: '', clear: false })).toEqual({ ok: true, value: undefined })
    expect(fromEdit(secret, { text: ' sk-1 ', clear: false })).toEqual({ ok: true, value: 'sk-1' })
    expect(fromEdit(secret, { text: 'ignored', clear: true })).toEqual({ ok: true, value: '' })
  })
})

describe('dirty tracking', () => {
  it('is clean right after loading', () => {
    for (const d of [num, flag, mode, text, list, json, secret, model]) expect(isDirty(d, toEdit(d)), d.key).toBe(false)
  })

  it('ignores formatting-only changes of JSON', () => {
    expect(isDirty(json, '{"--a":"b"}')).toBe(false)
    expect(isDirty(json, '{"--a":"c"}')).toBe(true)
  })

  it('treats invalid input as a change (so it is reported on save)', () => {
    expect(isDirty(num, '')).toBe(true)
    expect(isDirty(json, '{')).toBe(true)
  })

  it('detects a typed or cleared secret', () => {
    expect(isDirty(secret, { text: 'k', clear: false })).toBe(true)
    expect(isDirty(secret, { text: '  ', clear: false })).toBe(false)
    expect(isDirty(secret, { text: '', clear: true })).toBe(true)
  })

  it('compares against the default for "reset to default"', () => {
    expect(differsFromDefault(num, '12')).toBe(false)
    expect(differsFromDefault(num, '13')).toBe(true)
    expect(differsFromDefault(list, ['x'])).toBe(true)
    expect(differsFromDefault(list, defaultEdit(list))).toBe(false)
    expect(differsFromDefault({ ...secret, secretSet: true }, { text: '', clear: false })).toBe(true)
  })

  it('lists dirty keys only', () => {
    const defs = [num, flag, text]
    const drafts: Record<string, EditValue> = { 'auth.ttl': '24', 'llm.enabled': true, 'app.name': 'App2' }
    expect(dirtyKeys(defs, drafts)).toEqual(['auth.ttl', 'app.name'])
  })
})

describe('buildChanges', () => {
  it('sends only changed keys with normalised values', () => {
    const drafts: Record<string, EditValue> = {
      'auth.ttl': '24', 'llm.enabled': true, 'llm.mode': 'b', 'app.name': '  App  ',
      'llm.fallbacks': ['x', ' y '], 'ui.theme': '{"--a":"b"}', 'llm.apiKey': { text: 'k', clear: false }, 'llm.model': 'm/b',
    }
    const r = buildChanges([num, flag, mode, text, list, json, secret, model], drafts)
    expect(r.errors).toEqual({})
    expect(r.changes).toEqual({ 'auth.ttl': 24, 'llm.mode': 'b', 'llm.fallbacks': ['x', 'y'], 'llm.apiKey': 'k' })
  })

  it('collects an error per invalid key and does not send it', () => {
    const r = buildChanges([num, json], { 'auth.ttl': '0', 'ui.theme': '{' })
    expect(Object.keys(r.errors).sort()).toEqual(['auth.ttl', 'ui.theme'])
    expect(r.changes).toEqual({})
  })

  it('sends an empty string to clear a secret', () => {
    expect(buildChanges([secret], { 'llm.apiKey': { text: '', clear: true } }).changes).toEqual({ 'llm.apiKey': '' })
  })

  it('resets a value by sending the default', () => {
    const r = buildChanges([model], { 'llm.model': 'm/a' })
    expect(r.changes).toEqual({ 'llm.model': 'm/a' })
  })
})

describe('grouping and helpers', () => {
  it('groups by registry order and counts values stored in the database', () => {
    const g = groupSettings([num, flag, text, list, mode, { ...secret, secretSource: 'database' }])
    expect(g.map((x) => x.group)).toEqual(['auth', 'llm', 'general'])
    expect(g.find((x) => x.group === 'llm')!.modified).toBe(2)
    expect(g.find((x) => x.group === 'auth')!.modified).toBe(0)
  })

  it('extracts the key from the server message', () => {
    const keys = ['auth.ttl', 'llm.mode']
    expect(parseServerSettingError('auth.ttl: must be ≥ 1', keys)).toEqual({ key: 'auth.ttl', message: 'must be ≥ 1' })
    expect(parseServerSettingError("unknown setting 'llm.mode'", keys)).toEqual({ key: 'llm.mode', message: "unknown setting 'llm.mode'" })
    expect(parseServerSettingError('something: else', keys)).toEqual({ key: null, message: 'something: else' })
    expect(parseServerSettingError('internal error', keys)).toEqual({ key: null, message: 'internal error' })
  })

  it('formats defaults', () => {
    const l = { yes: 'yes', no: 'no', empty: 'empty' }
    expect(formatDefault(flag, l)).toBe('yes')
    expect(formatDefault(list, l)).toBe('empty')
    expect(formatDefault({ ...list, default: ['a', 'b'] }, l)).toBe('a, b')
    expect(formatDefault(json, l)).toBe('empty')
    expect(formatDefault(num, l)).toBe('12')
    expect(formatDefault(def({ key: 'k', type: 'string', default: '' }), l)).toBe('empty')
  })

  it('splits pasted lists', () => {
    expect(splitList('a, b\nc;;d\n')).toEqual(['a', 'b', 'c', 'd'])
  })

  it('searches key, label and help', () => {
    expect(settingMatches(num, 'AUTH', 'TTL', '')).toBe(true)
    expect(settingMatches(num, 'сессии', 'Время жизни сессии', '')).toBe(true)
    expect(settingMatches(num, 'zzz', 'a', 'b')).toBe(false)
    expect(settingMatches(num, '  ', 'a', 'b')).toBe(true)
  })
})
