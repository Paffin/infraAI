// Formatting and aggregation helpers for the OpenRouter pages.

import type { LlmModel, LlmUsageDay as UsageDay } from '@/api/types'

/** 1000000 -> "1M", 128000 -> "128k", 8192 -> "8.2k". */
export function fmtContext(n: number | null | undefined): string {
  if (n == null || !Number.isFinite(n)) return '—'
  if (n >= 1_000_000) return `${trim(n / 1_000_000)}M`
  if (n >= 1000) return `${trim(n / 1000)}k`
  return String(n)
}

function trim(x: number): string {
  return String(Math.round(x * 10) / 10)
}

/** USD price per million tokens: "$0.10", "$15", free -> "$0". */
export function fmtPrice(n: number | null | undefined): string {
  if (n == null || !Number.isFinite(n)) return '—'
  if (n === 0) return '$0'
  if (n < 0.01) return `$${n.toFixed(4)}`
  if (n < 10) return Number.isInteger(n) ? `$${n}` : `$${n.toFixed(2)}`
  return `$${Math.round(n)}`
}

/** USD amounts of the usage tables: more digits for tiny sums. */
export function fmtUsd(n: number | null | undefined): string {
  if (n == null || !Number.isFinite(n)) return '—'
  if (n === 0) return '$0'
  if (Math.abs(n) < 0.0001) return `$${n.toFixed(6)}`
  if (Math.abs(n) < 0.01) return `$${n.toFixed(4)}`
  return `$${n.toFixed(2)}`
}

/** y-axis label: enough decimals for the scale of the chart (max 0.05 -> "$0.025"). */
export function fmtAxis(n: number, max: number): string {
  if (n === 0) return '$0'
  const decimals = max >= 10 ? 0 : max >= 1 ? 1 : max >= 0.1 ? 2 : max >= 0.01 ? 3 : 4
  return `$${n.toFixed(decimals)}`
}

export interface ModelCaps { structured: boolean; responseFormat: boolean; temperature: boolean; maxTokens: boolean; reasoning: boolean }

/** What a model advertises in `supported_parameters`. */
export function modelCaps(m: Pick<LlmModel, 'structuredOutputs' | 'responseFormat' | 'supportedParameters'>): ModelCaps {
  const has = (...names: string[]) => names.some((n) => m.supportedParameters.includes(n))
  return {
    structured: m.structuredOutputs,
    responseFormat: m.responseFormat,
    temperature: has('temperature'),
    maxTokens: has('max_tokens', 'max_completion_tokens'),
    reasoning: has('reasoning', 'reasoning_effort', 'include_reasoning'),
  }
}

export type { LlmUsage as Usage, LlmUsageDay as UsageDay, LlmUsageByModel as UsageByModel, LlmUsageByUser as UsageByUser } from '@/api/types'

export function usageTotals(daily: UsageDay[]): { calls: number; errors: number; tokens: number; cost: number } {
  return daily.reduce(
    (a, d) => ({ calls: a.calls + d.calls, errors: a.errors + d.errors, tokens: a.tokens + d.promptTokens + d.completionTokens, cost: a.cost + d.costUsd }),
    { calls: 0, errors: 0, tokens: 0, cost: 0 },
  )
}

/** Month spend relative to the budget: ratio 0..1+ (null when there is no budget). */
export function budgetRatio(spend: number, budget: number): number | null {
  return budget > 0 ? spend / budget : null
}

/** Fill the days without calls so the bar chart has a continuous axis. `today` is injectable for tests. */
export function fillDays(daily: UsageDay[], days: number, today: Date = new Date()): UsageDay[] {
  const byDay = new Map(daily.map((d) => [d.day, d]))
  const out: UsageDay[] = []
  for (let i = days - 1; i >= 0; i--) {
    const d = new Date(Date.UTC(today.getUTCFullYear(), today.getUTCMonth(), today.getUTCDate() - i))
    const key = d.toISOString().slice(0, 10)
    out.push(byDay.get(key) ?? { day: key, calls: 0, errors: 0, promptTokens: 0, completionTokens: 0, costUsd: 0 })
  }
  return out
}

/** A "nice" upper bound for the y axis. */
export function niceMax(v: number): number {
  if (!(v > 0)) return 1
  const exp = Math.floor(Math.log10(v))
  const f = v / 10 ** exp
  const nf = f <= 1 ? 1 : f <= 2 ? 2 : f <= 5 ? 5 : 10
  return nf * 10 ** exp
}
