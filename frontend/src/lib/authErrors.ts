import { ApiError } from '@/api/client'
import { t } from '@/i18n'

// What the sign-in form and the form that replaces a temporary password say when the server refuses. The server's own text is English and is not for the
// screen of a Russian user: every code the server can answer on these two requests has a sentence of its own in both languages, and only a code nobody
// foresaw falls back to the server's text.

const unreachable = (e: ApiError): boolean => e.status === 0 || e.code === 'network'

/** `POST /auth/login`. A wrong password, an unknown address and a disabled account are one `401` on purpose, and so one sentence. */
export function signInError(e: unknown): string {
  if (!(e instanceof ApiError)) return t('error.network')
  if (unreachable(e)) return t('error.network')
  if (e.code === 'locked') return t('login.locked')
  if (e.code === 'rate_limited') return t('login.rateLimited')
  if (e.code === 'invalid_email') return t('login.invalidEmail')
  if (e.status === 401) return t('login.failed')
  return e.message
}

/** `POST /auth/password`. */
export function passwordChangeError(e: unknown): string {
  if (!(e instanceof ApiError)) return t('error.network')
  if (unreachable(e)) return t('error.network')
  if (e.code === 'locked') return t('login.change.locked')
  if (e.code === 'wrong_password') return t('login.change.wrong')
  return e.message
}
