import { t } from '@/i18n'
import type { JsonErr } from './json'

/** "Line 3, column 5: unexpected character }" in the current language. */
export function jsonErrorText(r: JsonErr): string {
  const msg = t(`admin.json.err.${r.code}`, r.params)
  return r.line ? t('admin.json.at', { line: r.line, column: r.column ?? 1, message: msg }) : msg
}
