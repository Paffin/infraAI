import type { Finding, Rule, Status } from '@/api/types'
import { has, lt, ltf, t } from '@/i18n'

/** Words for the engine's problem codes (`down,rto` → "service is down, RTO is not met"). */
function withProblemWords(vars: Record<string, unknown>): Record<string, unknown> {
  if (typeof vars.problems !== 'string' || !vars.problems) return vars
  const words = vars.problems.split(',').map((c) => (has('problem.' + c) ? t('problem.' + c) : c))
  return { ...vars, problems: words.join(', ') }
}

/** Findings whose fingerprint ends with `:req` come from an unmet explicit requirement. */
export function findingText(rule: Rule | undefined, f: Finding): string {
  if (!rule) return f.ruleId
  const vars = withProblemWords(f.vars ?? {})
  // a rule may word one kind of problem on its own (a version past its end of life reads differently from one about to be)
  const kindOfProblem = typeof vars.variant === 'string' ? rule.variantMessages?.[vars.variant] : undefined
  if (kindOfProblem) return ltf(kindOfProblem, vars)
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

/** The words for a fix: the rule's own label for its kind, or the general one. `vars`: the finding's variables with the fix's. */
export function fixKindLabel(rule: Rule | undefined, kind: string, vars: Record<string, unknown>): string {
  const tpl = rule?.fixLabels?.[kind]
  if (!tpl) return has('fix.' + kind) ? t('fix.' + kind) : kind
  return ltf(tpl, vars)
}

export function fixLabel(rule: Rule | undefined, fix: Finding['fixes'][number], f: Finding): string {
  return fixKindLabel(rule, fix.kind, { ...f.vars, ...fix.vars })
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
