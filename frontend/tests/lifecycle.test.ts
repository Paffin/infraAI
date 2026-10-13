import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import type { VersionSet } from '../src/api/types'
import { ageDays, cycleOf, formatDate, indexVersions, lifecycleOf, parseDate, recommended, verdictOf } from '../src/lib/lifecycle'

const tables: VersionSet[] = JSON.parse(readFileSync(join(process.cwd(), '..', 'backend', 'crates', 'engine', 'defaults', 'versions.json'), 'utf8'))
const byId = new Map(tables.map((t) => [t.id, t]))
const recorded: { cases: Record<string, unknown>[] } = JSON.parse(readFileSync(join(process.cwd(), 'tests', 'fixtures', 'lifecycle.json'), 'utf8'))

describe('dates', () => {
  it('are days since the epoch and roundtrip', () => {
    expect(parseDate('1970-01-01')).toBe(0)
    expect(parseDate('2000-03-01')).toBe(11017)
    expect(parseDate('2026-10-13')! - parseDate('2026-10-12')!).toBe(1)
    expect(parseDate('2024-03-01')! - parseDate('2024-02-28')!).toBe(2)
    expect(parseDate('nonsense')).toBeNull()
    expect(parseDate('2026-13-01')).toBeNull()
    expect(parseDate('2026-10-13T00:00:00Z')).toBe(parseDate('2026-10-13'))
    for (const d of ['1970-01-01', '1999-12-31', '2000-02-29', '2024-03-01', '2026-10-13', '2100-03-01']) expect(formatDate(parseDate(d)!)).toBe(d)
    expect(formatDate(-1)).toBe('1969-12-31')
  })
})

describe('the port agrees with the engine on the shipped release tables', () => {
  it('judges a version the way the engine does', () => {
    const verdicts = recorded.cases.filter((c) => 'lifecycle' in c)
    expect(verdicts.length).toBeGreaterThan(100)
    for (const c of verdicts) {
      const set = byId.get(c.table as string)!
      const got = lifecycleOf(set, c.version as string, parseDate(c.today as string)!)
      expect(got, `${c.table} ${c.version} on ${c.today}`).toEqual(c.lifecycle)
    }
  })

  it('recommends the line the engine recommends', () => {
    const recs = recorded.cases.filter((c) => 'recommended' in c)
    expect(recs.length).toBeGreaterThan(50)
    for (const c of recs) {
      const set = byId.get(c.table as string)!
      const got = recommended(set, parseDate(c.today as string)!, c.preferLts as boolean, c.matureDays as number)
      expect(got?.cycle ?? null, `${c.table} on ${c.today}`).toBe(c.recommended)
    }
  })
})

describe('lifecycle', () => {
  const pg = byId.get('postgresql')!
  const today = parseDate('2026-10-13')!

  it('assigns a version to the longest matching line', () => {
    const set: VersionSet = { id: 'x', source: '', asOf: '', cycles: [{ cycle: '1' }, { cycle: '1.31' }, { cycle: '16' }] }
    expect(cycleOf(set, '1.31.2')?.cycle).toBe('1.31')
    expect(cycleOf(set, 'v1.31')?.cycle).toBe('1.31')
    expect(cycleOf(set, '1.4')?.cycle).toBe('1')
    expect(cycleOf(set, '160')).toBeNull()
    expect(cycleOf(set, '')).toBeNull()
  })

  it('knows PostgreSQL 12 is over and 18 is not', () => {
    expect(lifecycleOf(pg, '12.19', today)?.phase).toBe('eol')
    expect(lifecycleOf(pg, '18.1', today)?.phase).not.toBe('eol')
    expect(lifecycleOf(pg, '99', today)).toBeNull()
  })

  it('turns a phase and a window into what the chip says', () => {
    const l = lifecycleOf(pg, '14', today)!
    expect(verdictOf(l, 0)).toBe(l.phase === 'maintenance' ? 'maintenance' : 'supported')
    expect(verdictOf({ ...l, phase: 'maintenance', daysToEol: 30 }, 180)).toBe('soon')
    expect(verdictOf({ ...l, phase: 'eol', daysToEol: -5 }, 180)).toBe('eol')
    expect(verdictOf(null, 180)).toBe('unknown')
  })

  it('measures the age of a table and indexes tables by id', () => {
    expect(ageDays({ ...pg, asOf: '2026-10-01' }, today)).toBe(12)
    const ix = indexVersions(tables, '2026-10-13', null)
    expect(ix.tables.get('kafka')?.name).toBe('Apache Kafka')
    expect(ix.settings.matureDays).toBe(90)
  })
})
