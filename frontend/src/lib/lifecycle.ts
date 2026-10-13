/**
 * Product lifecycle in the browser: what a release table says about the version a node runs.
 * A port of the engine's `lifecycle.rs`; `tests/lifecycle.test.ts` checks it against the engine's own verdicts.
 */
import type { Cycle, DateOrFlag, Lifecycle, LifecycleSettings, Phase, VersionSet } from '@/api/types'

/** Days since 1970-01-01 of a `YYYY-MM-DD` date (proleptic Gregorian); null when it does not parse. */
export function parseDate(s: string | null | undefined): number | null {
  if (!s) return null
  const m = /^\s*(-?\d{1,6})-(\d{1,2})-(\d{2})/.exec(s)
  if (!m) return null
  const y0 = Number(m[1])
  const mo = Number(m[2])
  const d = Number(m[3])
  if (mo < 1 || mo > 12 || d < 1 || d > 31) return null
  const y = mo <= 2 ? y0 - 1 : y0
  const era = Math.floor(y / 400)
  const yoe = y - era * 400
  const doy = Math.floor((153 * (mo > 2 ? mo - 3 : mo + 9) + 2) / 5) + d - 1
  const doe = yoe * 365 + Math.floor(yoe / 4) - Math.floor(yoe / 100) + doy
  return era * 146097 + doe - 719468
}

/** `YYYY-MM-DD` of a day count since 1970-01-01. */
export function formatDate(days: number): string {
  const z = days + 719468
  const era = Math.floor(z / 146097)
  const doe = z - era * 146097
  const yoe = Math.floor((doe - Math.floor(doe / 1460) + Math.floor(doe / 36524) - Math.floor(doe / 146096)) / 365)
  const y = yoe + era * 400
  const doy = doe - (365 * yoe + Math.floor(yoe / 4) - Math.floor(yoe / 100))
  const mp = Math.floor((5 * doy + 2) / 153)
  const d = doy - Math.floor((153 * mp + 2) / 5) + 1
  const m = mp < 10 ? mp + 3 : mp - 9
  return `${String(m <= 2 ? y + 1 : y).padStart(4, '0')}-${String(m).padStart(2, '0')}-${String(d).padStart(2, '0')}`
}

/** Has the date passed `today`? null = no date and no flag: unknown. */
export function reached(v: DateOrFlag | undefined | null, today: number): boolean | null {
  if (v === undefined || v === null) return null
  if (typeof v === 'boolean') return v
  const d = parseDate(v)
  return d === null ? null : d <= today
}

function dateOf(v: DateOrFlag | undefined | null): string | null {
  return typeof v === 'string' ? v : null
}

/** The release line a version string belongs to: the longest cycle that is a prefix of it on dot boundaries. */
export function cycleOf(set: VersionSet, version: string): Cycle | null {
  const v = version.trim().replace(/^[vV]+/, '')
  if (!v) return null
  let best: Cycle | null = null
  for (const c of set.cycles) {
    const hit = v === c.cycle || (v.startsWith(c.cycle) && ['.', '-', '_', ' '].includes(v.charAt(c.cycle.length)))
    if (hit && (!best || c.cycle.length > best.cycle.length)) best = c
  }
  return best
}

export function lifecycleOf(set: VersionSet, version: string, today: number): Lifecycle | null {
  const c = cycleOf(set, version)
  if (!c) return null
  const eol = reached(c.eol, today)
  const support = reached(c.support, today)
  let phase: Phase
  if (eol === true) phase = 'eol'
  else if (eol === false && support === true) phase = 'maintenance'
  else if (eol === false) phase = 'supported'
  else if (support === true) phase = 'maintenance'
  else if (support === false) phase = 'supported'
  else phase = 'unknown'
  const eolDate = dateOf(c.eol)
  const eolDay = parseDate(eolDate)
  return { cycle: c.cycle, phase, eol: eolDate, daysToEol: eolDay === null ? null : eolDay - today, latest: c.latest ?? null, lts: !!c.lts }
}

/**
 * The release line to move to: the newest one that still gets security fixes and has been out for `matureDays`, the
 * long-term-support ones first when `preferLts`; when nothing is that old yet, the newest maintained line.
 */
export function recommended(set: VersionSet, today: number, preferLts: boolean, matureDays: number): Cycle | null {
  const alive = set.cycles.filter((c) => reached(c.eol, today) !== true)
  alive.sort((a, b) => (b.releaseDate ?? '').localeCompare(a.releaseDate ?? '') || b.cycle.localeCompare(a.cycle))
  const mature = alive.filter((c) => {
    const d = parseDate(c.releaseDate)
    return d === null ? true : today - d >= matureDays
  })
  const pool = mature.length ? mature : alive
  if (preferLts) {
    const lts = pool.find((c) => c.lts)
    if (lts) return lts
  }
  return pool[0] ?? null
}

/** How many days old a table is (null when it does not say). */
export function ageDays(set: VersionSet, today: number): number | null {
  const d = parseDate(set.asOf)
  return d === null ? null : today - d
}

/** The settings of the engine when the server did not send them (an older backend). */
export const DEFAULT_LIFECYCLE: LifecycleSettings = { warnDays: 180, matureDays: 90, preferLts: true, staleDays: 120 }

export interface VersionIndex {
  today: number
  todayText: string
  settings: LifecycleSettings
  tables: Map<string, VersionSet>
}

export function indexVersions(tables: VersionSet[], todayText: string, settings?: LifecycleSettings | null): VersionIndex {
  return { today: parseDate(todayText) ?? 0, todayText, settings: settings ?? DEFAULT_LIFECYCLE, tables: new Map(tables.map((t) => [t.id, t])) }
}

/** Display state of a version for chips: the phase, or `soon` when the support ends within the warning window. */
export type Verdict = 'supported' | 'soon' | 'maintenance' | 'eol' | 'unknown'

export function verdictOf(l: Lifecycle | null, warnDays: number): Verdict {
  if (!l) return 'unknown'
  if (l.phase === 'eol') return 'eol'
  if (l.daysToEol !== null && l.daysToEol <= warnDays) return 'soon'
  return l.phase === 'maintenance' ? 'maintenance' : l.phase === 'supported' ? 'supported' : 'unknown'
}
