// A server that is busy says so: "429 rate_limited" with `Retry-After` (the heavy requests of one user, or of the whole replica, in flight at once: `limits.heavyConcurrentPerUser`,
// `limits.heavyConcurrentPerCore`). For a request the editor sends again and again by itself (the verdict after an edit, the failure being watched) that is not an error to show: it is
// "later". The answer is wanted, and the server says when to come back.
import { ApiError } from '@/api/client'

/**
 * How long to wait before asking again, in milliseconds, when `e` is the server being busy (`429 rate_limited` that carries `Retry-After`); `null` for every other failure,
 * which is shown as it always was. It is asked only of the requests the editor repeats by itself (the sign-in limit has the same code, and nobody retries a sign-in for the person).
 */
export function busyRetryMs(e: unknown): number | null {
  if (!(e instanceof ApiError) || e.status !== 429 || e.code !== 'rate_limited' || !e.retryAfter) return null
  return e.retryAfter * 1000
}
