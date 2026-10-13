import { ref } from 'vue'
import type { LText } from '@/api/types'
import ru from './ru'
import en from './en'
import ruAdmin from './ru.admin'
import enAdmin from './en.admin'
import ruPlacement from './ru.placement'
import enPlacement from './en.placement'
import ruCatalog from './ru.catalog'
import enCatalog from './en.catalog'

// Two flat dictionaries per language: the editor/common one and the admin panel one (kept apart so
// that they can evolve independently). Missing keys fall back to English, then to the key itself.
const dicts: Record<string, Record<string, string>> = {
  ru: { ...ru, ...ruAdmin, ...ruPlacement, ...ruCatalog },
  en: { ...en, ...enAdmin, ...enPlacement, ...enCatalog },
}

function initialLang(): string {
  try {
    const saved = localStorage.getItem('ia_lang')
    if (saved && dicts[saved]) return saved
  } catch { /* storage unavailable */ }
  const nav = (navigator.language || 'ru').slice(0, 2)
  return dicts[nav] ? nav : 'ru'
}

export const lang = ref<string>(initialLang())

export function setLang(l: string) {
  if (!dicts[l]) return
  lang.value = l
  try { localStorage.setItem('ia_lang', l) } catch { /* ignore */ }
  document.documentElement.lang = l
}

function pluralIndex(l: string, n: number): number {
  if (l === 'ru') {
    const m10 = n % 10
    const m100 = n % 100
    if (m10 === 1 && m100 !== 11) return 0
    if (m10 >= 2 && m10 <= 4 && (m100 < 12 || m100 > 14)) return 1
    return 2
  }
  return n === 1 ? 0 : 1
}

/** `35 с`, `15 мин`, `2 ч`, `3 дн`: the unit is chosen so that the number stays short. */
export function durationText(seconds: number | null | undefined): string {
  if (seconds == null || !Number.isFinite(seconds)) return '—'
  const s = Math.max(0, seconds)
  const short = (x: number) => String(Math.round(x * 10) / 10)
  if (s < 120) return t('common.seconds', { n: short(s) })
  if (s < 7200) return t('common.minutes', { n: short(s / 60) })
  if (s < 172800) return t('common.hours', { n: short(s / 3600) })
  return t('common.days', { n: short(s / 86400) })
}

function renderValue(v: unknown, format: string | undefined): string | null {
  if (v === undefined || v === null) return null
  if (format === 'dur' && typeof v === 'number') return durationText(v)
  if (format === 'pct' && typeof v === 'number') return `${Math.round(v * 100)}%`
  if (format?.startsWith('plural:') && typeof v === 'number') {
    // `{n|plural:площадка,площадки,площадок}` – the word form that agrees with the number
    const forms = format.slice('plural:'.length).split(',').map((x) => x.trim())
    return forms[Math.min(pluralIndex(lang.value, Math.round(Math.abs(v))), forms.length - 1)] ?? ''
  }
  if (typeof v === 'number') return Number.isInteger(v) ? String(v) : String(Math.round(v * 100) / 100)
  return String(v)
}

/** Replace `{name}` / `{name|dur}` / `{name|pct}` / `{name|plural:one,few,many}`; unknown placeholders stay visible. */
export function substitute(s: string, vars: Record<string, unknown>): string {
  return s.replace(/\{(\w+)(?:\|([^{}]+))?\}/g, (m, k: string, f: string | undefined) => (k in vars ? renderValue(vars[k], f) ?? m : m))
}

/** Translate `key`; `{name}` placeholders are replaced from `vars`; `a|b|c` picks a plural form by `vars.n`. */
export function t(key: string, vars?: Record<string, unknown>): string {
  let s = dicts[lang.value]?.[key] ?? dicts.en[key] ?? key
  if (vars) {
    if (s.includes('|') && typeof vars.n === 'number') {
      const forms = s.split(/\|(?![^{}]*\})/)
      s = forms[Math.min(pluralIndex(lang.value, vars.n), forms.length - 1)]
    }
    s = substitute(s, vars)
  }
  return s
}

export function has(key: string): boolean {
  return key in dicts[lang.value] || key in dicts.en
}

/** Resolve a server-side localised text (`string | {ru,en}`) with fallbacks. */
export function lt(x: LText | null | undefined, l: string = lang.value): string {
  if (x == null) return ''
  if (typeof x === 'string') return x
  return x[l] ?? x.en ?? x.ru ?? Object.values(x)[0] ?? ''
}

/** `lt` + `{placeholder}` substitution (unknown placeholders stay visible). */
export function ltf(x: LText | null | undefined, vars: Record<string, unknown>, l: string = lang.value): string {
  return substitute(lt(x, l), vars)
}

export function useI18n() {
  return { t, lt, ltf, lang, setLang }
}

export const supportedLangs = Object.keys(dicts)
