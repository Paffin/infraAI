import type { Availability, AvailabilityCause } from '@/api/types'
import { t } from '@/i18n'
import { fmtDuration } from '@/lib/format'
import { yearlyDowntime } from '@/lib/nines'

/** The order the causes are listed in when they cost the same: from what a design can do least about to what it can do most. */
export const CAUSE_ORDER: AvailabilityCause[] = ['geo', 'site', 'rack', 'isolation', 'link', 'provider', 'unit', 'instance']

/** How often a failure happens: `2× a year`, `once in 4 years`. */
export function rateText(perYear: number): string {
  if (!Number.isFinite(perYear) || perYear <= 0) return '—'
  if (perYear >= 0.95) {
    const n = perYear >= 10 ? Math.round(perYear) : Math.round(perYear * 10) / 10
    return t('avail.rate.perYear', { n })
  }
  return t('avail.rate.everyYears', { n: Math.max(2, Math.round(1 / perYear)) })
}

/** Downtime a year, short: `53 min`, `8.8 h`. */
export const downtimeText = (seconds: number): string => fmtDuration(seconds)

/** What a target allows: `99.9%` → `8.8 h a year`. */
export function allowedText(target: number, daysPerYear = 365.25): string {
  return t('avail.allowed', { v: downtimeText(yearlyDowntime(target, daysPerYear)) })
}

/** Where the estimate stands against the target: the share of the downtime budget it uses. */
export type Standing = 'none' | 'within' | 'close' | 'over'

export function standing(a: Availability): Standing {
  if (a.target === null || a.budgetSecondsPerYear === null) return 'none'
  const used = a.budgetSecondsPerYear > 0 ? a.downtimeSecondsPerYear / a.budgetSecondsPerYear : Infinity
  if (used > 1) return 'over'
  if (used > a.basis.warnPercent / 100) return 'close'
  return 'within'
}

/** Share of the downtime budget used (0…, can pass 1); `null` without a target. */
export function budgetUsed(a: Availability): number | null {
  if (a.target === null || a.budgetSecondsPerYear === null) return null
  return a.budgetSecondsPerYear > 0 ? a.downtimeSecondsPerYear / a.budgetSecondsPerYear : Infinity
}

export const causeLabel = (c: AvailabilityCause) => t('avail.cause.' + c)
