import { beforeAll, describe, expect, it } from 'vitest'
import { setLang } from '../src/i18n'
import { effectiveProps, functionGroups, kindPropDefs, nodeSubtitle, tableIdOf, traitsOf, variantOf, behaviorOf } from '../src/lib/catalog'
import { findingText, fixLabel } from '../src/lib/findings'
import { indexVersions } from '../src/lib/lifecycle'
import { versionRefs, viewVersion } from '../src/lib/product'
import { fixture, realCatalog, realIndex, realRule, realVersions } from './fixtures'

const ix = realIndex()
const vx = indexVersions(realVersions(), '2026-10-13', null)
const kind = (id: string) => ix.kinds.get(id)!

beforeAll(() => setLang('ru'))

describe('variants of a product', () => {
  it('are assumed (the default one) until a node names another', () => {
    expect(variantOf(kind('postgresql'), undefined)?.id).toBe('patroni')
    expect(variantOf(kind('postgresql'), 'rds')?.id).toBe('rds')
    expect(variantOf(kind('postgresql'), 'no-such')?.id).toBe('patroni')
    expect(variantOf(kind('users'), 'x')).toBeUndefined()
  })

  it('change what the engine assumes about a node', () => {
    const eff = effectiveProps(ix, 'postgresql', { variant: 'rds' })
    expect(eff.failover).toBe('auto')
    expect(eff.failoverSeconds).toBe(90)
    expect(effectiveProps(ix, 'postgresql', { variant: 'streaming' }).failover).toBe('manual')
    // what the user declared wins over what the variant assumes
    expect(effectiveProps(ix, 'postgresql', { variant: 'rds', failoverSeconds: 45 }).failoverSeconds).toBe(45)
  })

  it('add traits and change behavior', () => {
    expect(traitsOf(kind('postgresql'), variantOf(kind('postgresql'), 'rds')).has('managed')).toBe(true)
    expect(traitsOf(kind('postgresql'), variantOf(kind('postgresql'), 'patroni')).has('managed')).toBe(false)
    expect(behaviorOf(kind('vault'), variantOf(kind('vault'), 'raft'))).toBe('consensus')
    expect(behaviorOf(kind('vault'), variantOf(kind('vault'), undefined))).toBe('primary_replica')
  })

  it('hide the properties that belong to other variants', () => {
    const keys = (v?: string) => kindPropDefs(ix, 'postgresql', v).map((p) => p.key)
    expect(keys()).toContain('dcs')
    expect(keys('patroni')).toContain('dcs')
    expect(keys('rds')).not.toContain('dcs')
    expect(keys('streaming')).not.toContain('dcs')
  })

  it('show up in the subtitle of a card', () => {
    const node = (props: Record<string, string>) => ({ id: 'm', kind: 'monitoring', name: 'm', zoneId: null, group: null, role: null, props })
    expect(nodeSubtitle(ix, node({}))).toBe('Prometheus HA (пара)')
    expect(nodeSubtitle(ix, node({ variant: 'victoria' }))).toBe('VictoriaMetrics (кластер)')
  })
})

describe('the library by function', () => {
  const cat = realCatalog()

  it('lists every visible product exactly once', () => {
    const seen = cat.categories.flatMap((c) => functionGroups(cat, c.id).flatMap((g) => g.kinds.map((k) => k.id)))
    const visible = cat.kinds.filter((k) => !k.hidden).map((k) => k.id)
    expect(seen.sort()).toEqual(visible.sort())
  })

  it('groups the products that replace each other', () => {
    const rdbms = functionGroups(cat, 'data').find((g) => g.id === 'rdbms')!
    expect(rdbms.kinds.map((k) => k.id)).toEqual(expect.arrayContaining(['postgresql', 'mysql', 'mariadb', 'mssql', 'oracle']))
    expect(functionGroups(cat, 'edge').find((g) => g.id === 'l7')!.kinds.map((k) => k.id)).toEqual(expect.arrayContaining(['l7', 'nginx', 'haproxy', 'envoy', 'traefik']))
  })

  it('keeps a product without a declared function instead of losing it', () => {
    const custom = { ...cat, kinds: [...cat.kinds, { ...cat.kinds[0], id: 'mine', function: 'nonexistent', category: 'data' }] }
    expect(functionGroups(custom, 'data').flatMap((g) => g.kinds).some((k) => k.id === 'mine')).toBe(true)
  })

  it('filters', () => {
    const only = functionGroups(cat, 'data', (k) => k.id === 'mysql')
    expect(only.flatMap((g) => g.kinds.map((k) => k.id))).toEqual(['mysql'])
  })
})

describe('release tables of a node', () => {
  const arch = fixture('report-versions').architecture
  const node = (id: string) => arch.nodes.find((n) => n.id === id)!

  it('are chosen by the variant, then by the kind', () => {
    expect(tableIdOf(kind('postgresql'), variantOf(kind('postgresql'), 'rds'))).toBe('amazon-rds-postgresql')
    expect(tableIdOf(kind('postgresql'), variantOf(kind('postgresql'), undefined))).toBe('postgresql')
    expect(tableIdOf(kind('monitoring'), variantOf(kind('monitoring'), 'victoria'))).toBeNull()
    expect(tableIdOf(kind('nginx'), undefined)).toBe('nginx')
    expect(tableIdOf(kind('apache_httpd'), undefined)).toBe('apache')
  })

  it('include the operating system named by the node', () => {
    const refs = versionRefs(ix, vx, 'vm_pool', node('app').props)
    expect(refs.map((r) => [r.prop, r.set.id])).toEqual([['version', 'esxi'], ['osVersion', 'ubuntu']])
    expect(versionRefs(ix, vx, 'vm_pool', node('app2').props).map((r) => r.set.id)).toEqual(['proxmox_ve', 'debian'].slice(0, 0).concat(['esxi', 'debian']))
    expect(versionRefs(ix, vx, 'postgresql', node('pg1').props).map((r) => r.set.id)).toEqual(['postgresql'])
    expect(versionRefs(ix, null, 'postgresql', node('pg1').props)).toEqual([])
  })

  it('are judged the way the engine judges them', () => {
    const pg = versionRefs(ix, vx, 'postgresql', node('pg1').props)[0]
    const v = viewVersion(vx, pg.set, '12.19')
    expect(v.verdict).toBe('eol')
    expect(v.lifecycle?.eol).toBe('2024-11-21')
    expect(v.recommended?.cycle).toBe('18')
    expect(viewVersion(vx, pg.set, '17.2').verdict).toBe('supported')
    expect(viewVersion(vx, pg.set, '99').verdict).toBe('unknown')
    expect(viewVersion(vx, pg.set, undefined).lifecycle).toBeNull()
  })
})

describe('what the engine says about versions', () => {
  const { report } = fixture('report-versions')
  const rule = realRule('VER-01')!
  const find = (id: string) => report.findings.find((f) => f.id === id)!

  it('is worded per kind of problem, with the product named', () => {
    expect(findingText(rule, find('VER-01:pg1:version'))).toBe('«PostgreSQL primary»: PostgreSQL 12.19 — линия 12 больше не получает исправлений безопасности (с 2024-11-21). Рекомендуемая линия: 18.')
    expect(findingText(rule, find('VER-01:app:osVersion'))).toContain('Ubuntu 20.04')
    expect(findingText(rule, find('VER-01:kafka:version'))).toContain('не найдена в таблице выпусков')
  })

  it('offers the move to the recommended line', () => {
    const f = find('VER-01:pg1:version')
    expect(f.fixes[0].kind).toBe('set_version')
    expect(fixLabel(rule, f.fixes[0], f)).toBe('Перейти на 18 (PostgreSQL, «PostgreSQL primary»)')
  })

  it('reports members on different lines', () => {
    const f = report.findings.find((x) => x.ruleId === 'VER-02')!
    expect(findingText(realRule('VER-02'), f)).toContain('12, 17')
  })
})
