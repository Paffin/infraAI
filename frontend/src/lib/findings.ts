import type { Finding, Rule, Status } from '@/api/types'
import { has, lt, ltf, t } from '@/i18n'

/** Findings whose fingerprint ends with `:req` come from an unmet explicit requirement. */
export function findingText(rule: Rule | undefined, f: Finding): string {
  if (!rule) return f.ruleId
  const vars = f.vars ?? {}
  if (f.id.endsWith(':lost') && f.status !== 'no_data' && rule.dataLostMessage) return ltf(rule.dataLostMessage, vars)
  if (f.status === 'no_data') {
    const m = rule.noDataMessage ?? rule.message
    return ltf(f.id.endsWith(':req') && rule.requirementMessage ? rule.requirementMessage : m, vars)
  }
  if (f.id.endsWith(':req') && rule.requirementMessage) return ltf(rule.requirementMessage, vars)
  return ltf(rule.message, vars)
}

export function findingTitle(rule: Rule | undefined, f: Finding): string {
  return rule ? lt(rule.title) : f.ruleId
}

export function fixLabel(rule: Rule | undefined, fix: Finding['fixes'][number], f: Finding): string {
  const tpl = rule?.fixLabels?.[fix.kind]
  if (!tpl) return has('fix.' + fix.kind) ? t('fix.' + fix.kind) : fix.kind
  return ltf(tpl, { ...f.vars, ...fix.vars })
}

export function statusLabel(s: Status): string {
  return t('checks.statuses.' + s)
}

export function statusTagClass(s: Status): string {
  return s === 'critical' ? 'tag-crit' : s === 'risk' ? 'tag-risk' : s === 'no_data' ? 'tag-nodata' : s === 'ok' ? 'tag-ok' : 'tag-nodata'
}

export function statusColor(s: Status): string {
  return s === 'critical' ? 'var(--crit)' : s === 'risk' ? 'var(--risk-dot)' : s === 'no_data' ? 'var(--info-dot)' : s === 'ok' ? 'var(--ok-dot)' : 'var(--faint)'
}

/** Worst status first; used to sort findings and to colour node dots. */
export const statusRank: Record<Status, number> = { critical: 0, risk: 1, no_data: 2, ok: 3, na: 4 }
