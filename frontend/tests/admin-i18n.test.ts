import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import en from '../src/i18n/en.admin'
import ru from '../src/i18n/ru.admin'
import { CONFIG_KINDS } from '../src/admin/lib/configKinds'

const ADMIN_DIR = join(__dirname, '../src/admin')

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((f) => {
    const p = join(dir, f)
    return statSync(p).isDirectory() ? files(p) : /\.(vue|ts)$/.test(f) ? [p] : []
  })
}

const vars = (s: string) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort()

describe('admin i18n', () => {
  it('has the same keys in Russian and English', () => {
    expect(Object.keys(ru).sort()).toEqual(Object.keys(en).sort())
  })

  it('uses the same {placeholders} in both languages', () => {
    for (const k of Object.keys(ru)) expect(vars(ru[k]), k).toEqual(vars(en[k]))
  })

  it('only uses admin.* keys and never leaves a string empty', () => {
    for (const [k, v] of Object.entries({ ...ru })) {
      expect(k.startsWith('admin.'), k).toBe(true)
      expect(v.trim(), k).not.toBe('')
    }
    for (const [k, v] of Object.entries({ ...en })) expect(v.trim(), k).not.toBe('')
  })

  it('has no exclamation marks (tone of the product)', () => {
    for (const v of [...Object.values(ru), ...Object.values(en)]) expect(v).not.toMatch(/!/)
  })

  it('defines every key referenced by the components', () => {
    const used = new Set<string>()
    for (const f of files(ADMIN_DIR)) {
      for (const m of readFileSync(f, 'utf8').matchAll(/['"`](admin\.[A-Za-z0-9_.]+)['"`]/g)) used.add(m[1])
    }
    expect(used.size).toBeGreaterThan(200)
    const missing = [...used].filter((k) => !(k in ru) || !(k in en))
    expect(missing).toEqual([])
  })

  it('defines the dynamic key families', () => {
    const need: string[] = []
    for (const g of ['general', 'auth', 'llm', 'jobs', 'limits', 'retention', 'observability', 'lifecycle']) need.push(`admin.group.${g}`)
    for (const k of CONFIG_KINDS) need.push(`admin.kind.${k.kind}.title`, `admin.kind.${k.kind}.desc`)
    for (const c of ['string', 'number', 'min', 'max', 'enum', 'boolean', 'list', 'json', 'json_object']) need.push(`admin.settings.err.${c}`)
    for (const c of ['unexpected_token', 'unexpected_end', 'expected_comma', 'expected_colon', 'expected_key', 'trailing_comma', 'trailing_data', 'bad_number', 'bad_escape', 'unterminated_string', 'control_char', 'too_deep', 'syntax']) need.push(`admin.json.err.${c}`)
    for (const s of ['database', 'environment', 'none']) need.push(`admin.settings.secret.${s}`)
    for (const r of ['planner', 'fast']) need.push(`admin.llm.role.${r}`)
    for (const c of ['llm_not_configured', 'llm_disabled', 'llm_budget', 'llm_auth', 'llm_credits', 'llm_rate_limited', 'llm_http', 'llm_timeout', 'llm_empty', 'llm_truncated']) need.push(`admin.llm.pingCode.${c}`)
    for (const r of ['viewer', 'editor', 'admin']) need.push(`admin.users.role.${r}`, `admin.users.roleHint.${r}`)
    for (const p of ['self', 'lastAdmin']) need.push(`admin.users.protect.${p}`)
    for (const b of ['json', 'shape', 'empty']) need.push(`admin.backup.bad.${b}`)
    for (const j of ['queued', 'running', 'succeeded', 'failed', 'cancelled']) need.push(`admin.system.job.${j}`)
    for (const s of ['assumptions', 'scoring', 'scenarios', 'cost', 'export']) need.push(`admin.engine.sec.${s}`)
    expect(need.filter((k) => !(k in ru) || !(k in en))).toEqual([])
  })
})
