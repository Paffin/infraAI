import { afterEach, describe, expect, it } from 'vitest'
import { ApiError } from '../src/api/client'
import { setLang, t } from '../src/i18n'
import { passwordChangeError, signInError } from '../src/lib/authErrors'

// The real dictionaries and the real error class; the server's texts below are the ones it sends (English), which must never reach the screen.

afterEach(() => setLang('ru'))

const refusals: { name: string; error: ApiError; key: string }[] = [
  { name: 'a wrong password, an unknown address, a disabled account', error: new ApiError(401, 'unauthorized', 'invalid email or password'), key: 'login.failed' },
  { name: 'a lock', error: new ApiError(429, 'locked', 'account is temporarily locked after repeated failures'), key: 'login.locked' },
  { name: 'the limit of an address', error: new ApiError(429, 'rate_limited', 'too many sign-in attempts, try again later'), key: 'login.rateLimited' },
  { name: 'an address that is nobody\'s', error: new ApiError(400, 'invalid_email', 'invalid email'), key: 'login.invalidEmail' },
  { name: 'no connection', error: new ApiError(0, 'network', 'Failed to fetch'), key: 'error.network' },
]

describe('what the sign-in form says', () => {
  for (const lang of ['ru', 'en']) {
    for (const r of refusals) {
      it(`${r.name}, in ${lang}: a sentence of the dictionary, not the server's English`, () => {
        setLang(lang)
        const said = signInError(r.error)
        expect(said).toBe(t(r.key))
        expect(said).not.toBe(r.error.message)
        expect(said).not.toBe(r.key)
      })
    }
  }

  it('says something about a failure that is not an ApiError, and passes on a code nobody foresaw', () => {
    setLang('en')
    expect(signInError(new TypeError('x'))).toBe(t('error.network'))
    expect(signInError(new ApiError(500, 'internal', 'internal error'))).toBe('internal error')
  })

  it('has a sentence of its own in each language for every refusal (the two are different texts)', () => {
    for (const r of refusals) {
      setLang('ru')
      const ru = signInError(r.error)
      setLang('en')
      expect(signInError(r.error), r.name).not.toBe(ru)
    }
  })
})

describe('what the form that replaces a temporary password says', () => {
  const wrong = new ApiError(400, 'wrong_password', 'current password is incorrect')
  const locked = new ApiError(429, 'locked', 'too many wrong passwords, try again later')

  it('translates the wrong current password and the lock that follows many of them', () => {
    for (const lang of ['ru', 'en']) {
      setLang(lang)
      expect(passwordChangeError(wrong)).toBe(t('login.change.wrong'))
      expect(passwordChangeError(wrong)).not.toBe(wrong.message)
      expect(passwordChangeError(locked)).toBe(t('login.change.locked'))
      expect(passwordChangeError(locked)).not.toBe(locked.message)
      expect(passwordChangeError(new ApiError(0, 'network', 'Failed to fetch'))).toBe(t('error.network'))
    }
  })

  it('does not say "the account is locked": the sign-in is not what is locked here', () => {
    setLang('en')
    expect(passwordChangeError(locked)).not.toBe(t('login.locked'))
  })

  it('passes on a code nobody foresaw', () => {
    expect(passwordChangeError(new ApiError(400, 'weak_password', 'password is too simple'))).toBe('password is too simple')
  })
})
