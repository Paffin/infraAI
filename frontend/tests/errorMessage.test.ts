import { afterEach, describe, expect, it } from 'vitest'
import { ApiError, http } from '../src/api/client'
import { setLang, t } from '../src/i18n'
import { errorMessage } from '../src/lib/format'

// `errorMessage` is what 70-odd places of the application put on the screen when something failed (toasts, the export window, the versions window, the chat,
// the save status). The browser's own sentence for a request that got no answer is English, differs from one browser to the next and tells nobody what to do.

afterEach(() => setLang('ru'))

const browserSentences = ['Failed to fetch', 'NetworkError when attempting to fetch resource.', 'Load failed', 'network error']

describe('a request that got no answer', () => {
  it('is said in the words of the page, in each language, whatever sentence the browser used', () => {
    for (const lang of ['ru', 'en']) {
      setLang(lang)
      for (const raw of browserSentences) {
        const said = errorMessage(new ApiError(0, 'network', raw))
        expect(said, `${lang}: ${raw}`).toBe(t('error.network'))
        expect(said).not.toContain(raw)
      }
    }
    setLang('ru')
    expect(errorMessage(new ApiError(0, 'network', 'Failed to fetch'))).toBe('Нет связи с сервером')
    setLang('en')
    expect(errorMessage(new ApiError(0, 'network', 'Failed to fetch'))).toBe('Cannot reach the server')
  })

  it('follows the language when it is asked again (nothing is frozen into the error)', () => {
    const e = new ApiError(0, 'network', 'Failed to fetch')
    setLang('ru')
    const ru = errorMessage(e)
    setLang('en')
    expect(errorMessage(e)).not.toBe(ru)
  })

  it('is the failure of the real client with no server to talk to', async () => {
    // jsdom has no server at this address: the request fails the way it fails without a network, through the real `fetch` and the real client
    const failure = await http.get('/projects').then(() => null, (e: unknown) => e)
    expect(failure).toBeInstanceOf(ApiError)
    expect((failure as ApiError).code).toBe('network')
    expect(errorMessage(failure)).toBe(t('error.network'))
    expect(errorMessage(failure)).not.toMatch(/fetch|parse|url/i)
  })
})

describe('everything else is as it was', () => {
  it('an answer of the server keeps the sentence of the server', () => {
    expect(errorMessage(new ApiError(500, 'internal', 'internal error'))).toBe('internal error')
    expect(errorMessage(new ApiError(409, 'conflict', 'the project was changed'))).toBe('the project was changed')
    expect(errorMessage(new ApiError(422, 'invalid', 'architecture is invalid'))).toBe('architecture is invalid')
  })

  it('a status 0 that the client did not call a network failure is not rewritten', () => {
    expect(errorMessage(new ApiError(0, 'other', 'something else'))).toBe('something else')
  })

  it('a plain error, an abort, an object with a message, a string and nothing at all', () => {
    expect(errorMessage(new Error('boom'))).toBe('boom')
    expect(errorMessage(new DOMException('aborted', 'AbortError'))).toBe('aborted')
    expect(errorMessage({ message: 'from an object' })).toBe('from an object')
    expect(errorMessage('just text')).toBe('just text')
    expect(errorMessage(undefined)).toBe('undefined')
    expect(errorMessage(null)).toBe('null')
  })

  it('a plain error that happens to say what the browser says is left alone: only the client\'s own mark makes it a network failure', () => {
    expect(errorMessage(new TypeError('Failed to fetch'))).toBe('Failed to fetch')
  })
})
