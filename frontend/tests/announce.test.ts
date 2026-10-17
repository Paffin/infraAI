import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Report } from '../src/api/types'
import { setLang } from '../src/i18n'
import { announce, clearAnnouncement, describeChange, diffReports, liveMessage } from '../src/lib/announce'
import { fixture, realRule } from './fixtures'

// The reports are the engine's own (recorded, tests/fixtures/README.md): a clean one, one with two critical findings of one rule, one with critical findings and risks of three rules.
const clean = fixture('report-three-az').report
const spof = fixture('report-spof').report
const overloaded = fixture('report-overloaded').report
const versions = fixture('report-versions').report
const gaps = fixture('report-gaps').report

const rules = { get: (id: string) => realRule(id) }
const titleOf = (id: string, lang: 'ru' | 'en') => {
  const title = realRule(id)!.title as { ru: string; en: string }
  return title[lang]
}

/** A copy of a recorded report with its findings changed by `fn` (a report is read-only data; the copy is the "after" of an edit). */
function edited(r: Report, fn: (r: Report) => void): Report {
  const c = structuredClone(r)
  fn(c)
  return c
}

beforeEach(() => setLang('ru'))
afterEach(() => { setLang('ru'); clearAnnouncement(); vi.useRealTimers() })

describe('what changed between two reports', () => {
  it('tells the findings that came, the findings that went and the change in the checks without data', () => {
    const d = diffReports(clean, spof)
    expect(d.introduced.map((f) => f.id).sort()).toEqual(['HA-SPOF-01:gslb', 'HA-SPOF-01:waf'])
    expect(d.resolved).toEqual([])
    expect(diffReports(spof, clean).resolved.map((f) => f.id).sort()).toEqual(['HA-SPOF-01:gslb', 'HA-SPOF-01:waf'])
    expect(diffReports(spof, clean).introduced).toEqual([])
    expect(diffReports(clean, gaps).noData).toBe(gaps.counts.noData - clean.counts.noData)
  })

  it('puts the critical findings before the risks', () => {
    const levels = diffReports(clean, overloaded).introduced.map((f) => f.status)
    expect(levels).toEqual([...levels].sort((a, b) => (a === b ? 0 : a === 'critical' ? -1 : 1)))
    expect(levels.filter((s) => s === 'critical')).toHaveLength(3)
    expect(levels.filter((s) => s === 'risk')).toHaveLength(5)
  })

  it('counts a risk that became critical as new, and a critical that became a risk as eased (it is still open, so it is neither new nor resolved)', () => {
    const worse = edited(overloaded, (r) => { r.findings.find((f) => f.id === 'CAP-01:redis')!.status = 'critical' })
    expect(diffReports(overloaded, worse).introduced.map((f) => f.id)).toEqual(['CAP-01:redis'])
    expect(diffReports(overloaded, worse).eased).toEqual([])
    const back = diffReports(worse, overloaded)
    expect(back).toMatchObject({ introduced: [], resolved: [] })
    expect(back.eased.map((f) => f.id)).toEqual(['CAP-01:redis'])
    // a finding that stays where it was is none of them
    expect(diffReports(overloaded, overloaded)).toMatchObject({ introduced: [], resolved: [], eased: [], noData: 0 })
  })

  it('does not count a finding that was accepted (waived) as open', () => {
    const waived = edited(spof, (r) => { for (const f of r.findings) f.accepted = { reason: 'known' } })
    expect(diffReports(clean, waived).introduced).toEqual([])
    expect(diffReports(spof, waived).resolved).toHaveLength(2)
  })
})

describe('the words for a change', () => {
  it('says nothing when nothing a person has to know changed: a report compared with itself, or with the report of a card that was only moved', () => {
    expect(describeChange(spof, spof, rules)).toBeNull()
    expect(describeChange(clean, clean, rules)).toBeNull()
    expect(describeChange(versions, structuredClone(versions), rules)).toBeNull()
  })

  it('says what became critical and what it is about, in Russian', () => {
    const text = describeChange(clean, spof, rules)!
    expect(text).toContain('2 новые критичные находки')
    expect(text).toContain(titleOf('HA-SPOF-01', 'ru'))
    expect(text).not.toMatch(/устранен/)
  })

  it('says it in English, with the English plural', () => {
    setLang('en')
    const text = describeChange(clean, spof, rules)!
    expect(text).toContain('2 new critical findings')
    expect(text).toContain(titleOf('HA-SPOF-01', 'en'))
    expect(describeChange(spof, clean, rules)).toBe('2 findings resolved.')
  })

  it('says what was resolved', () => {
    expect(describeChange(spof, clean, rules)).toBe('2 находки устранены.')
    const one = edited(spof, (r) => { r.findings = r.findings.filter((f) => f.id !== 'HA-SPOF-01:waf') })
    expect(describeChange(spof, one, rules)).toBe('1 находка устранена.')
  })

  it('says what was eased: a critical finding that became a risk is not silence', () => {
    const worse = edited(overloaded, (r) => { r.findings.find((f) => f.id === 'CAP-01:redis')!.status = 'critical' })
    expect(describeChange(worse, overloaded, rules)).toBe('1 критичная находка стала риском.')
    setLang('en')
    expect(describeChange(worse, overloaded, rules)).toBe('1 critical finding is now a risk.')
    // two at once, the Russian plural; and said together with what was resolved
    setLang('ru')
    const two = edited(overloaded, (r) => { for (const f of r.findings.filter((x) => x.status === 'risk').slice(0, 2)) f.status = 'critical' })
    expect(describeChange(two, overloaded, rules)).toBe('2 критичные находки стали рисками.')
    const withOldFinding = edited(two, (r) => { r.findings.push(structuredClone(spof.findings.find((f) => f.id === 'HA-SPOF-01:waf')!)) })
    expect(describeChange(withOldFinding, overloaded, rules)).toBe('1 находка устранена, 2 критичные находки стали рисками.')
  })

  it('names at most two rules, the worst first, and counts the rest', () => {
    const ru = describeChange(clean, overloaded, rules)!
    expect(ru).toContain('3 новые критичные находки, 5 новых рисков')
    // CAP-01 has critical findings, CAP-02 and HA-CACHE-02 only risks: two are named, one is left
    expect(ru).toContain(`«${titleOf('CAP-01', 'ru')}»`)
    expect(ru).toContain(' и ещё 1')
    expect((ru.match(/«/g) ?? []).length).toBe(2)
    setLang('en')
    const en = describeChange(clean, overloaded, rules)!
    expect(en).toContain('3 new critical findings, 5 new risks')
    expect(en).toContain(' and 1 more')
    expect((en.match(/"/g) ?? []).length).toBe(4)
    // three findings of one rule are one name
    expect((describeChange(clean, versions, rules)!.match(/"/g) ?? []).length).toBe(4)
  })

  it('agrees the words with the number', () => {
    const one = edited(spof, (r) => { r.findings = r.findings.filter((f) => f.id !== 'HA-SPOF-01:waf') })
    expect(describeChange(clean, one, rules)).toContain('1 новая критичная находка')
    expect(describeChange(clean, spof, rules)).toContain('2 новые критичные находки')
    expect(describeChange(clean, versions, rules)).toContain('7 новых критичных находок, 7 новых рисков')
    const many = edited(versions, (r) => { for (const f of r.findings) if (f.status === 'risk') f.status = 'critical' })
    expect(describeChange(clean, many, rules)).toContain('14 новых критичных находок')
    setLang('en')
    expect(describeChange(clean, one, rules)).toContain('1 new critical finding.')
    expect(describeChange(clean, versions, rules)).toContain('7 new critical findings, 7 new risks')
  })

  it('announces a swap: one finding gone and another one there leave the counts where they were and are the very edit to tell about', () => {
    const swapped = edited(spof, (r) => {
      const f = r.findings.find((x) => x.id === 'HA-SPOF-01:waf')!
      f.id = 'HA-SPOF-01:cdn'
      f.subjects = { nodes: ['cdn'], edges: [], zones: [] }
    })
    expect(swapped.counts).toEqual(spof.counts)
    expect(swapped.findings.filter((f) => f.status === 'critical')).toHaveLength(spof.findings.filter((f) => f.status === 'critical').length)
    expect(describeChange(spof, swapped, rules)).toBe(`1 новая критичная находка, 1 находка устранена. Что именно: «${titleOf('HA-SPOF-01', 'ru')}».`)
  })

  it('announces a change in the checks that cannot be made for lack of data, and only that', () => {
    const more = describeChange(clean, gaps, rules)!
    expect(more).toBe(`Не хватает данных ещё для ${gaps.counts.noData - clean.counts.noData} проверок.`)
    expect(describeChange(gaps, clean, rules)).toBe(`Данных теперь хватает для ${gaps.counts.noData - clean.counts.noData} проверок.`)
    const one = edited(clean, (r) => { r.counts.noData = 1 })
    expect(describeChange(clean, one, rules)).toBe('Не хватает данных ещё для 1 проверки.')
    setLang('en')
    expect(describeChange(clean, one, rules)).toBe('Data is missing for 1 more check.')
  })

  it('writes the name of a rule it does not know as its id', () => {
    const unknown = { get: () => undefined }
    expect(describeChange(clean, spof, unknown)).toContain('«HA-SPOF-01»')
  })
})

describe('the live region', () => {
  it('holds the last message', () => {
    announce('first')
    expect(liveMessage.value).toBe('first')
    announce('second')
    expect(liveMessage.value).toBe('second')
    clearAnnouncement()
    expect(liveMessage.value).toBe('')
  })

  it('takes a message that is the same as the one there away first, so that it is read again', async () => {
    vi.useFakeTimers()
    announce('same')
    announce('same')
    expect(liveMessage.value).toBe('')
    await vi.advanceTimersByTimeAsync(200)
    expect(liveMessage.value).toBe('same')
  })

  it('does not let an old message come back over a new one', async () => {
    vi.useFakeTimers()
    announce('same')
    announce('same')
    announce('other')
    await vi.advanceTimersByTimeAsync(200)
    expect(liveMessage.value).toBe('other')
  })
})
