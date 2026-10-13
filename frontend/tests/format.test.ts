import { describe, expect, it } from 'vitest'
import { durationText, lang, setLang, substitute, t, lt, ltf } from '../src/i18n'
import { diffIsEmpty, diffLines } from '../src/lib/diff'
import { splitSeconds, toSeconds, newId } from '../src/lib/format'
import { renderSubtitle } from '../src/lib/catalog'
import { scoreTone } from '../src/lib/score'
import { sameScenario, shortZone } from '../src/lib/sim'

describe('i18n', () => {
  it('formats durations in the active language', () => {
    setLang('ru')
    expect(durationText(35)).toBe('35 с')
    expect(durationText(900)).toBe('15 мин')
    expect(durationText(7200)).toBe('2 ч')
    expect(durationText(259200)).toBe('3 дн')
    expect(durationText(null)).toBe('—')
    setLang('en')
    expect(durationText(900)).toBe('15 min')
    setLang('ru')
  })

  it('substitutes placeholders with formats and keeps unknown ones visible', () => {
    setLang('ru')
    expect(substitute('RTO {a|dur}, {r|pct}, {x}', { a: 900, r: 0.96 })).toBe('RTO 15 мин, 96%, {x}')
    expect(ltf({ ru: 'за {s|dur}', en: 'in {s|dur}' }, { s: 90 })).toBe('за 90 с')
  })

  it('picks Russian plural forms', () => {
    setLang('ru')
    expect(t('projects.nodes', { n: 1 })).toBe('1 узел')
    expect(t('projects.nodes', { n: 3 })).toBe('3 узла')
    expect(t('projects.nodes', { n: 11 })).toBe('11 узлов')
    expect(t('projects.nodes', { n: 21 })).toBe('21 узел')
  })

  it('falls back from a missing language and a missing key', () => {
    expect(lt({ en: 'only english' }, 'ru')).toBe('only english')
    expect(t('no.such.key')).toBe('no.such.key')
    expect(lang.value).toBe('ru')
  })
})

describe('helpers', () => {
  it('splits and joins durations', () => {
    expect(splitSeconds(900)).toEqual({ value: 15, unit: 'minutes' })
    expect(splitSeconds(7200)).toEqual({ value: 2, unit: 'hours' })
    expect(splitSeconds(45)).toEqual({ value: 45, unit: 'seconds' })
    expect(toSeconds(15, 'minutes')).toBe(900)
  })

  it('creates distinct ids', () => {
    const ids = new Set(Array.from({ length: 200 }, () => newId('n')))
    expect(ids.size).toBe(200)
  })

  it('drops subtitle segments with unset properties', () => {
    expect(renderSubtitle('{replicas} pods · HPA {hpaMin}–{hpaMax}', { replicas: 12 })).toBe('12 pods')
    expect(renderSubtitle('{replicas} pods · HPA {hpaMin}–{hpaMax}', { replicas: 12, hpaMin: 6, hpaMax: 30 })).toBe('12 pods · HPA 6–30')
  })

  it('bands the score', () => {
    expect(scoreTone(92)).toBe('ok')
    expect(scoreTone(70)).toBe('risk')
    expect(scoreTone(40)).toBe('crit')
    expect(scoreTone(null)).toBe('none')
  })

  it('compares scenarios and shortens zone names', () => {
    expect(sameScenario({ type: 'zone', zoneId: 'a' }, { type: 'zone', zoneId: 'a' })).toBe(true)
    expect(sameScenario({ type: 'zone', zoneId: 'a' }, { type: 'zone', zoneId: 'b' })).toBe(false)
    expect(shortZone('ЦОД-1 · Москва')).toBe('ЦОД-1')
  })
})

describe('diff lines', () => {
  const empty = { added: [], removed: [], changed: [] }
  it('flattens an architecture diff', () => {
    const d = { requirements: [{ path: 'rpsPeak', from: 100, to: 200 }], zones: empty, nodes: { added: [{ id: 'a', name: 'Redis' }], removed: [{ id: 'b', name: 'Old' }], changed: [{ id: 'c', name: 'PG', changes: [{ path: 'props.replicas', from: 1, to: 3 }] }] }, edges: empty }
    const lines = diffLines(d)
    expect(lines.map((l) => l.sign)).toEqual(['~', '+', '-', '~'])
    expect(lines[3].text).toContain('props.replicas: 1 → 3')
    expect(diffIsEmpty(d)).toBe(false)
    expect(diffIsEmpty({ requirements: [], zones: empty, nodes: empty, edges: empty })).toBe(true)
  })
})

describe('semantic zoom', () => {
  it('switches the level of detail at the configured zoom levels', async () => {
    const { lodOf, LOD_COMPACT_BELOW, LOD_MINI_BELOW } = await import('../src/lib/lod')
    expect(lodOf(1)).toBe('full')
    expect(lodOf(LOD_COMPACT_BELOW)).toBe('full')
    expect(lodOf(LOD_COMPACT_BELOW - 0.01)).toBe('compact')
    expect(lodOf(LOD_MINI_BELOW)).toBe('compact')
    expect(lodOf(LOD_MINI_BELOW - 0.01)).toBe('mini')
    expect(LOD_MINI_BELOW).toBeLessThan(LOD_COMPACT_BELOW)
  })
})
