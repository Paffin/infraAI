import { ApiError } from '@/api/client'
import { lang, t } from '@/i18n'

export { durationText as fmtDuration } from '@/i18n'

export function round(n: number, digits = 1): number {
  const k = 10 ** digits
  return Math.round(n * k) / k
}

export function fmtNumber(n: number | null | undefined): string {
  if (n == null || !Number.isFinite(n)) return '—'
  return new Intl.NumberFormat(lang.value).format(n)
}

export function fmtMoney(n: number | null | undefined, symbol: string): string {
  if (n == null || !Number.isFinite(n)) return '—'
  return `${new Intl.NumberFormat(lang.value, { maximumFractionDigits: 0 }).format(Math.round(n))} ${symbol}`
}

export function fmtPct(ratio: number | null | undefined): string {
  if (ratio == null || !Number.isFinite(ratio)) return '—'
  return `${Math.round(ratio * 100)}%`
}

const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ['year', 31536000], ['month', 2592000], ['day', 86400], ['hour', 3600], ['minute', 60],
]

export function fmtRelative(iso: string | null | undefined): string {
  if (!iso) return '—'
  const ts = Date.parse(iso)
  if (Number.isNaN(ts)) return '—'
  const diff = Math.round((ts - Date.now()) / 1000)
  const rtf = new Intl.RelativeTimeFormat(lang.value, { numeric: 'auto' })
  for (const [unit, secs] of UNITS) {
    if (Math.abs(diff) >= secs) return rtf.format(Math.round(diff / secs), unit)
  }
  return rtf.format(0, 'second')
}

export function fmtDateTime(iso: string | null | undefined): string {
  if (!iso) return '—'
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return '—'
  return new Intl.DateTimeFormat(lang.value, { dateStyle: 'medium', timeStyle: 'short' }).format(d)
}

/** Parse "15 мин" / "30s" / "2h" style inputs into seconds (used by the requirements form). */
export function toSeconds(value: number, unit: 'seconds' | 'minutes' | 'hours'): number {
  return unit === 'hours' ? value * 3600 : unit === 'minutes' ? value * 60 : value
}

export function splitSeconds(seconds: number): { value: number; unit: 'seconds' | 'minutes' | 'hours' } {
  if (seconds > 0 && seconds % 3600 === 0) return { value: seconds / 3600, unit: 'hours' }
  if (seconds > 0 && seconds % 60 === 0) return { value: seconds / 60, unit: 'minutes' }
  return { value: seconds, unit: 'seconds' }
}

export function clamp(n: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, n))
}

let counter = 0
/** Short readable id with a prefix, unique within a session and (practically) across users. */
export function newId(prefix: string): string {
  counter += 1
  const rnd = Math.random().toString(36).slice(2, 6)
  return `${prefix}-${Date.now().toString(36).slice(-4)}${rnd}${counter.toString(36)}`
}

/** The longest name a project can have: what the server accepts (`api/projects.rs`). A check of the input of the API, not a setting. */
export const PROJECT_NAME_MAX = 200

/** The name proposed for a copy of a project ("<name> (copy)"): it is the NAME that is cut when the whole would be too long for the server, never the mark of the copy. */
export function copyName(name: string): string {
  const whole = t('projects.duplicate.name', { name })
  const chars = Array.from(name)
  const over = Array.from(whole).length - PROJECT_NAME_MAX
  return over <= 0 ? whole : t('projects.duplicate.name', { name: chars.slice(0, Math.max(0, chars.length - over)).join('') })
}

/**
 * What a failure says on the screen. A request that got no answer (no connection, a server that is down) throws the browser's own sentence ("Failed to fetch",
 * "NetworkError when attempting to fetch resource"): English, different in each browser, and no help to anybody. The client marks such a failure with the code
 * `network`, and it is said in the words of the page.
 */
export function errorMessage(e: unknown): string {
  if (e instanceof ApiError && e.code === 'network') return t('error.network')
  if (e && typeof e === 'object' && 'message' in e) return String((e as { message: unknown }).message)
  return String(e)
}
