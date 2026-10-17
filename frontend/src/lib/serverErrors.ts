// The refusals of the server that a person reads, in the words of the page. The server answers every one with a stable `code`, the numbers and names the sentence needs
// in `details`, and an English sentence for scripts and the log (docs/API.md, "Errors"): the interface never shows that sentence for a code it knows, because it is
// English for a person who reads Russian. A code this file does not know is shown by the server's sentence, as before.
import { ApiError } from '@/api/client'
import { has, t } from '@/i18n'

const isNum = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v)
const record = (v: unknown): Record<string, unknown> => (v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : {})

/** The word for a field a text was refused in (`details.field`): the page's own label, or the name the server gave when the page has none. */
function fieldLabel(field: unknown): string {
  const name = typeof field === 'string' ? field : ''
  const key = `error.field.${name}`
  return name && has(key) ? t(key) : name
}

/**
 * The text of `kind` for one value of a setting the server refused: `reasonCode` and `params` of the `invalid_setting` answer. The bounds, the options and the types
 * are the texts of the form itself (`admin.settings.err.*`, which the page also uses before it asks the server), the rest are `error.setting.*`; a code that has no
 * text yet is shown by the English sentence the server gave.
 */
function settingReason(details: Record<string, unknown>): string {
  const code = typeof details.reasonCode === 'string' ? details.reasonCode : ''
  const params = record(details.params)
  const english = typeof details.reason === 'string' ? details.reason : code
  let said: string
  switch (code) {
    case 'below_min':
      said = t('admin.settings.err.min', params)
      break
    case 'above_max':
      said = t('admin.settings.err.max', params)
      break
    case 'not_an_option':
      said = t('admin.settings.err.enum', params)
      break
    case 'wrong_type': {
      const kinds: Record<string, string> = { string: 'string', number: 'number', boolean: 'boolean', list: 'list', object: 'json_object' }
      const kind = kinds[String(params.expected)]
      said = kind ? t(`admin.settings.err.${kind}`) : english
      break
    }
    default:
      said = code && has(`error.setting.${code}`) ? t(`error.setting.${code}`, params) : english
  }
  // an entry of a list: the one that is wrong is named
  return typeof params.value === 'string' && params.value && said !== english ? t('error.setting.entry', { value: params.value, reason: said }) : said
}

export interface SettingRefusal {
  key: string
  /** Why, in the language of the page (no key in front). */
  message: string
}

/** A setting value the server did not accept (`400 invalid_setting`), as the key it was for and the reason in the language of the page; `null` for any other failure. */
export function settingRefusal(e: unknown): SettingRefusal | null {
  if (!(e instanceof ApiError) || e.code !== 'invalid_setting') return null
  const d = record(e.details)
  if (typeof d.key !== 'string' || !d.key) return null
  return { key: d.key, message: settingReason(d) }
}

/**
 * What a refusal of the server says, in the language of the page, for the codes the page knows; `null` for every other failure (the caller shows what the server wrote).
 *
 * `field_too_long` / `invalid_name` (a text past its limit), `ops_too_many`, `request_timeout` (504), `payload_too_large` (413), `rate_limited` of the heavy-request gate
 * (429 with `details.scope`; the sign-in limit has the same code and no scope, and says what it says), `invalid_setting`.
 */
export function serverRefusal(e: unknown): string | null {
  if (!(e instanceof ApiError)) return null
  const d = record(e.details)
  switch (e.code) {
    case 'field_too_long':
      return isNum(d.max) && isNum(d.actual) ? t('error.fieldTooLong', { field: fieldLabel(d.field), max: d.max, actual: d.actual }) : null
    case 'invalid_name':
      return isNum(d.max) && isNum(d.actual) ? t('error.invalidName', { max: d.max, actual: d.actual }) : null
    case 'ops_too_many':
      return isNum(d.max) && isNum(d.actual) ? t('error.opsTooMany', { max: d.max, actual: d.actual }) : null
    case 'request_timeout':
      return t('error.requestTimeout')
    case 'payload_too_large':
      return t('error.payloadTooLarge')
    case 'rate_limited': {
      if (d.scope !== 'user' && d.scope !== 'replica') return null
      const seconds = e.retryAfter
      if (d.scope === 'user' && isNum(d.limit) && seconds) return t('error.busy.user', { limit: d.limit, seconds })
      return seconds ? t('error.busy', { seconds }) : t('error.busy.soon')
    }
    case 'invalid_setting': {
      const s = settingRefusal(e)
      return s ? `${s.key}: ${s.message}` : null
    }
    default:
      return null
  }
}
