import { beforeEach, describe, expect, it } from 'vitest'
import { setLang } from '../src/i18n'
import { archFingerprint, effectChips, recommendationLabel } from '../src/lib/recommend'
import { planFixture, realRule } from './fixtures'
import type { Effect } from '../src/api/types'

// the plan the engine really makes for two-dc-witness with `gslb` and `waf` at one replica
const { architecture, plan } = planFixture('plan-spof')

/** The first recommendation of the plan with some of its figures changed. */
const withEffect = (over: Partial<Effect>): Effect => ({ ...plan.items[0].effect, ...over })

describe('what a recommendation does, in the figures of the engine', () => {
  beforeEach(() => setLang('ru'))

  it('lists what moved, and only that', () => {
    const first = plan.items[0]
    const chips = effectChips(first.effect, '₽')
    const ids = chips.map((c) => c.id)
    expect(ids).toContain('critical')
    expect(ids).toContain('score')
    expect(ids).toContain('availability')
    expect(ids).toContain('cost')
    expect(ids).not.toContain('risk') // the risks did not change: 0 → 0
    expect(ids).not.toContain('introduced')
    const critical = chips.find((c) => c.id === 'critical')!
    expect(critical.text).toBe(`критичных ${first.effect.critical.before} → ${first.effect.critical.after}`)
    expect(critical.tone).toBe('good')
  })

  it('writes the availability with its nines, and the downtime it means in the tooltip', () => {
    const chip = effectChips(plan.items[0].effect, '₽').find((c) => c.id === 'availability')!
    expect(chip.text).toMatch(/^доступность 99\.\d+% → 99\.\d+%$/)
    expect(chip.text).not.toContain('0.99')
    expect(chip.title).toMatch(/^Простой в год: .+ → .+$/)
    expect(chip.tone).toBe('good')
  })

  it('says what a fix costs with its sign, and a saving as good news', () => {
    const cost = effectChips(plan.items[0].effect, '₽').find((c) => c.id === 'cost')!
    expect(cost.text).toMatch(/^\+.*₽ в мес$/)
    expect(cost.tone).toBe('plain') // paying for resilience is not bad, and not good either
    const saving = effectChips(withEffect({ cost: { before: 5000, after: 3800 } }), '₽').find((c) => c.id === 'cost')!
    expect(saving.text).toMatch(/^−.*₽ в мес$/)
    expect(saving.tone).toBe('good')
  })

  it('does not hide what a fix makes worse', () => {
    const worse = effectChips(withEffect({ risk: { before: 2, after: 3 }, introduced: ['X:y'], resolved: [], availability: { before: 0.9995, after: 0.9991 } }), '₽')
    expect(worse.find((c) => c.id === 'risk')).toMatchObject({ tone: 'bad', text: 'рисков 2 → 3' })
    expect(worse.find((c) => c.id === 'introduced')).toMatchObject({ tone: 'bad', text: 'новых замечаний: 1' })
    expect(worse.find((c) => c.id === 'availability')?.tone).toBe('bad')
  })

  it('names the rules of the findings it closes in the tooltip of the critical chip', () => {
    const chip = effectChips(withEffect({ resolved: ['AV-SLO-01:slo', 'HA-SPOF-01:gslb', 'HA-SPOF-01:waf'] }), '₽').find((c) => c.id === 'critical')!
    expect(chip.title).toBe('Закрывает: AV-SLO-01, HA-SPOF-01') // each rule once
    expect(effectChips(withEffect({ resolved: [] }), '₽').find((c) => c.id === 'critical')?.title).toBeUndefined()
  })

  it('an availability that reads the same on both sides is not a change', () => {
    const same = effectChips(withEffect({ availability: { before: 0.99951, after: 0.99954 } }), '₽')
    expect(same.find((c) => c.id === 'availability')).toBeUndefined()
  })

  it('speaks English when the language is English', () => {
    setLang('en')
    const chips = effectChips(plan.items[0].effect, '₽')
    expect(chips.find((c) => c.id === 'critical')?.text).toMatch(/^critical \d+ → \d+$/)
    expect(chips.find((c) => c.id === 'cost')?.text).toMatch(/a month$/)
  })
})

describe('the label of a recommended fix', () => {
  beforeEach(() => setLang('en'))

  it('is the rule’s own wording of that kind of fix, with the variables of the finding and the fix', () => {
    const first = plan.items[0]
    const label = recommendationLabel(realRule(first.ruleId), first)
    expect(label.length).toBeGreaterThan(3)
    expect(label).not.toContain('{') // every placeholder was filled from `vars`
    expect(label).toContain(String(first.vars.replicas)) // "…to 2 instances"
  })

  it('falls back to the general words for the kind, and to the kind itself', () => {
    expect(recommendationLabel(undefined, { kind: 'add_backup', vars: {} })).toBe('add backups')
    expect(recommendationLabel(undefined, { kind: 'brand_new_kind', vars: {} })).toBe('brand_new_kind')
  })
})

describe('the plan is for one design, wherever its cards lie', () => {
  it('moving a card does not make it another design', () => {
    const moved = structuredClone(architecture)
    for (const n of moved.nodes) n.ui = { x: (n.ui?.x ?? 0) + 40, y: (n.ui?.y ?? 0) - 20 }
    for (const z of moved.zones) z.ui = { ...z.ui, x: (z.ui?.x ?? 0) + 40 }
    expect(archFingerprint(moved)).toBe(archFingerprint(architecture))
  })

  it('changing what is in it does', () => {
    const base = archFingerprint(architecture)
    const more = structuredClone(architecture)
    more.nodes[0].props = { ...more.nodes[0].props, replicas: 7 }
    expect(archFingerprint(more)).not.toBe(base)
    const fewer = structuredClone(architecture)
    fewer.edges.pop()
    expect(archFingerprint(fewer)).not.toBe(base)
    const asked = structuredClone(architecture)
    asked.requirements = { ...asked.requirements, availabilitySlo: 99.999 }
    expect(archFingerprint(asked)).not.toBe(base)
  })
})

describe('the recorded plan itself', () => {
  it('is ranked by what the fix closes, and the bundle is what applying them all does', () => {
    expect(plan.items.length).toBeGreaterThanOrEqual(2)
    const closed = plan.items.map((r) => r.effect.critical.before - r.effect.critical.after)
    expect(closed).toEqual([...closed].sort((a, b) => b - a))
    expect(plan.bundle?.ids).toEqual(plan.items.map((r) => r.id))
    expect(plan.bundle!.effect.critical.after).toBeLessThan(plan.items[0].effect.critical.after)
    expect(plan.bundle!.ops.length).toBe(plan.items.reduce((n, r) => n + r.ops.length, 0))
  })
})
