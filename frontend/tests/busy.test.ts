import { describe, expect, it } from 'vitest'
import { ApiError } from '../src/api/client'
import { busyRetryMs } from '../src/lib/busy'

// A server that is busy answers `429 rate_limited` with `Retry-After` (docs/API.md, "Heavy requests"). Whether a request is "later" is decided here, from the error the
// client made of the answer; how the editor behaves (it shows that it is calculating, asks again after the Retry-After the real gate sent, and shows the verdict) is
// checked against the real gate and the real editor in e2e/busy.spec.ts. Nothing in this file plays the part of the server: the errors are the client's own data type.

const busy = (retryAfter?: number) => new ApiError(429, 'rate_limited', 'you already have 3 heavy requests running', { scope: 'user', limit: 3 }, retryAfter)

describe('busyRetryMs: when a failure is "later"', () => {
  it('is the Retry-After the server sent, in milliseconds', () => {
    expect(busyRetryMs(busy(3))).toBe(3000)
    expect(busyRetryMs(busy(1))).toBe(1000)
  })

  it('is nothing when the server did not say when: such a refusal is shown, not retried', () => {
    expect(busyRetryMs(busy())).toBeNull()
    expect(busyRetryMs(busy(0))).toBeNull()
  })

  it('is nothing for any other failure', () => {
    expect(busyRetryMs(new ApiError(429, 'locked', 'locked', undefined, 30))).toBeNull() // another 429
    expect(busyRetryMs(new ApiError(503, 'unavailable', 'draining', undefined, 5))).toBeNull() // another status
    expect(busyRetryMs(new ApiError(504, 'request_timeout', 'took too long'))).toBeNull()
    expect(busyRetryMs(new ApiError(0, 'network', 'offline'))).toBeNull()
    expect(busyRetryMs(new Error('boom'))).toBeNull()
    expect(busyRetryMs(undefined)).toBeNull()
  })
})
