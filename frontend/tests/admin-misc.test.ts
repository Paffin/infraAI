import { describe, expect, it } from 'vitest'
import type { AdminUser, AuditEvent } from '../src/api/types'
import { actionTone, hasDetails, mergeEvents, nextCursor } from '../src/admin/lib/audit'
import { exportFileName, parseImport } from '../src/admin/lib/backup'
import { budgetRatio, fillDays, fmtAxis, fmtContext, fmtPrice, fmtUsd, modelCaps, niceMax, usageTotals } from '../src/admin/lib/llm'
import { activeNavId, guardKey, NAV } from '../src/admin/lib/nav'
import { jobTone, shortDbVersion, sortJobs, splitUptime } from '../src/admin/lib/system'
import { activeAdmins, isLocked, protection } from '../src/admin/lib/users'

const user = (o: Partial<AdminUser>): AdminUser => ({ id: 'u', email: 'e', name: '', role: 'editor', disabled: false, mustChangePassword: false, createdAt: '', lastLoginAt: null, lockedUntil: null, ...o })

describe('llm helpers', () => {
  it('formats context length and prices', () => {
    expect(fmtContext(1_000_000)).toBe('1M')
    expect(fmtContext(128_000)).toBe('128k')
    expect(fmtContext(8192)).toBe('8.2k')
    expect(fmtContext(null)).toBe('—')
    expect(fmtPrice(0.1)).toBe('$0.10')
    expect(fmtPrice(3)).toBe('$3')
    expect(fmtPrice(15)).toBe('$15')
    expect(fmtPrice(0)).toBe('$0')
    expect(fmtPrice(null)).toBe('—')
    expect(fmtUsd(0.0488424)).toBe('$0.05')
    expect(fmtUsd(0.0008)).toBe('$0.0008')
    expect(fmtUsd(0.000004)).toBe('$0.000004')
    expect(fmtAxis(0.025, 0.05)).toBe('$0.025')
    expect(fmtAxis(5, 10)).toBe('$5')
  })

  it('reads capabilities from supported_parameters', () => {
    const caps = modelCaps({ structuredOutputs: true, responseFormat: true, supportedParameters: ['max_tokens', 'reasoning'] })
    expect(caps).toEqual({ structured: true, responseFormat: true, temperature: false, maxTokens: true, reasoning: true })
    expect(modelCaps({ structuredOutputs: false, responseFormat: false, supportedParameters: ['temperature'] }).temperature).toBe(true)
  })

  it('computes budget ratio, totals and a continuous day axis', () => {
    expect(budgetRatio(5, 10)).toBe(0.5)
    expect(budgetRatio(5, 0)).toBeNull()
    const daily = [
      { day: '2026-10-11', calls: 2, errors: 1, promptTokens: 10, completionTokens: 5, costUsd: 0.5 },
      { day: '2026-10-12', calls: 3, errors: 0, promptTokens: 20, completionTokens: 5, costUsd: 1 },
    ]
    expect(usageTotals(daily)).toEqual({ calls: 5, errors: 1, tokens: 40, cost: 1.5 })
    const filled = fillDays(daily, 4, new Date(Date.UTC(2026, 9, 12, 15)))
    expect(filled.map((d) => d.day)).toEqual(['2026-10-09', '2026-10-10', '2026-10-11', '2026-10-12'])
    expect(filled[0].costUsd).toBe(0)
    expect(filled[3].calls).toBe(3)
  })

  it('picks a nice axis maximum', () => {
    expect(niceMax(0.0488)).toBe(0.05)
    expect(niceMax(7)).toBe(10)
    expect(niceMax(1.2)).toBe(2)
    expect(niceMax(0)).toBe(1)
  })
})

describe('backup helpers', () => {
  it('names the export file with a sortable local timestamp', () => {
    expect(exportFileName(new Date(2026, 9, 2, 3, 4, 5))).toBe('infra-architect-config-2026-10-02T03-04-05.json')
  })

  it('parses an export and summarises it', () => {
    const text = JSON.stringify({ format: 'infra-architect-config/1', settings: { a: 1 }, items: [{ kind: 'rule', id: 'x', data: {} }, { kind: 'rule', id: 'y', data: {} }, { kind: 'prompt', id: 'z', data: {} }] })
    const r = parseImport(text)
    expect(r.ok).toBe(true)
    if (r.ok) {
      expect(r.summary).toEqual({ format: 'infra-architect-config/1', settings: 1, items: 3, byKind: [['prompt', 1], ['rule', 2]], formatOk: true })
      expect(Object.keys(r.payload)).toEqual(['settings', 'items'])
    }
  })

  it('rejects broken or empty payloads', () => {
    expect(parseImport('{')).toEqual({ ok: false, reason: 'json' })
    expect(parseImport('[1]')).toEqual({ ok: false, reason: 'shape' })
    expect(parseImport('{"settings": [], "items": []}')).toEqual({ ok: false, reason: 'shape' })
    expect(parseImport('{"settings": {}, "items": []}')).toEqual({ ok: false, reason: 'empty' })
    const loose = parseImport('{"items": [{"kind":"rule","id":"a","data":{}}]}')
    expect(loose.ok && loose.summary.formatOk).toBe(false)
  })
})

describe('users', () => {
  it('locks controls of the current user and of the last active admin', () => {
    const me = user({ id: 'me', role: 'admin' })
    const other = user({ id: 'o', role: 'admin' })
    const viewer = user({ id: 'v', role: 'viewer' })
    expect(protection(me, 'me', activeAdmins([me, other, viewer]))).toBe('self')
    expect(protection(other, 'me', activeAdmins([me, other, viewer]))).toBeNull()
    expect(protection(other, 'me', activeAdmins([other]))).toBe('lastAdmin')
    expect(protection(viewer, 'me', activeAdmins([other]))).toBeNull()
    expect(activeAdmins([me, user({ id: 'd', role: 'admin', disabled: true })])).toEqual([me])
  })

  it('knows when an account is still locked', () => {
    const now = Date.parse('2026-10-12T10:00:00Z')
    expect(isLocked({ lockedUntil: '2026-10-12T10:05:00Z' }, now)).toBe(true)
    expect(isLocked({ lockedUntil: '2026-10-12T09:55:00Z' }, now)).toBe(false)
    expect(isLocked({ lockedUntil: null }, now)).toBe(false)
  })
})

describe('audit helpers', () => {
  const ev = (id: number, details: Record<string, unknown> = {}): AuditEvent => ({ id, at: '', actor: '', action: 'x', targetType: '', targetId: '', details, ip: '' })

  it('paginates with the smallest id as cursor and merges without duplicates', () => {
    expect(nextCursor([ev(30), ev(29), ev(28)])).toBe(28)
    expect(nextCursor([])).toBeUndefined()
    expect(mergeEvents([ev(3), ev(2)], [ev(2), ev(1)]).map((e) => e.id)).toEqual([3, 2, 1])
  })

  it('classifies actions and details', () => {
    expect(actionTone('auth.login_failed')).toBe('crit')
    expect(actionTone('config.delete')).toBe('risk')
    expect(actionTone('user.reset_password')).toBe('risk')
    expect(actionTone('settings.update')).toBe('accent')
    expect(actionTone('project.create')).toBe('nodata')
    expect(hasDetails(ev(1, { a: 1 }))).toBe(true)
    expect(hasDetails(ev(1))).toBe(false)
  })
})

describe('system helpers', () => {
  it('shortens the database version and splits uptime', () => {
    expect(shortDbVersion('PostgreSQL 16.15 (Ubuntu 16.15-0ubuntu0.24.04.1) on x86_64-pc-linux-gnu')).toBe('PostgreSQL 16.15')
    expect(shortDbVersion('')).toBe('—')
    expect(splitUptime(93784)).toEqual({ d: 1, h: 2, m: 3 })
    expect(splitUptime(-5)).toEqual({ d: 0, h: 0, m: 0 })
  })

  it('orders job statuses and tones them', () => {
    expect(sortJobs([{ status: 'failed' }, { status: 'weird' }, { status: 'queued' }, { status: 'succeeded' }]).map((j) => j.status)).toEqual(['queued', 'succeeded', 'failed', 'weird'])
    expect(jobTone('succeeded')).toBe('ok')
    expect(jobTone('failed')).toBe('crit')
    expect(jobTone('cancelled')).toBe('nodata')
  })
})

describe('navigation', () => {
  it('resolves the active item and the unsaved-changes scope', () => {
    expect(activeNavId('catalog', undefined)).toBe('catalog/kind')
    expect(activeNavId('catalog', 'zone_type')).toBe('catalog/zone_type')
    expect(activeNavId('rules', undefined)).toBe('rules')
    expect(guardKey('settings', 'auth')).toBe(guardKey('settings', 'llm'))
    expect(guardKey('catalog', 'kind')).not.toBe(guardKey('catalog', 'edge_type'))
    expect(guardKey('llm', undefined)).not.toBe(guardKey('settings', undefined))
  })

  it('links every item to its own section', () => {
    const items = NAV.flatMap((g) => g.items.flatMap((i) => [i, ...(i.children ?? [])]))
    expect(new Set(items.map((i) => i.id)).size).toBe(items.length)
    for (const i of items) expect(i.to.startsWith('/admin/')).toBe(true)
  })
})
