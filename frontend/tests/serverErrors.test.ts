import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'
import { ApiError } from '../src/api/client'
import { has, setLang } from '../src/i18n'
import { errorMessage } from '../src/lib/format'
import { serverRefusal, settingRefusal } from '../src/lib/serverErrors'

// The server answers a refusal with a stable `code`, the numbers and names in `details`, and an English sentence for scripts. A person who reads Russian must not be shown
// that sentence: the page says it in its own words. The answers here are built the way the server builds them (tests/budgets of the Rust crate assert these very shapes
// on the real router: `details.reasonCode`, `params`, `field`, `max`, `actual`, `scope`, `limit`, `Retry-After`).

afterEach(() => setLang('ru'))

const both = (check: (lang: 'ru' | 'en') => void) => {
  for (const lang of ['ru', 'en'] as const) {
    setLang(lang)
    check(lang)
  }
}
const err = (status: number, code: string, message: string, details?: unknown, retryAfter?: number) => new ApiError(status, code, message, details, retryAfter)

describe('a text past its limit', () => {
  const tooLong = err(400, 'field_too_long', 'description is longer than 20000 characters', { field: 'description', max: 20000, actual: 20001 })

  it('names the field and the numbers in the language of the page', () => {
    setLang('ru')
    expect(errorMessage(tooLong)).toBe('Поле «Описание» слишком длинное: не больше 20000 знаков, сейчас 20001')
    setLang('en')
    expect(errorMessage(tooLong)).toBe('The field “Description” is too long: at most 20000 characters, now 20001')
  })

  it('knows every field the server caps, and shows the name of one it does not know', () => {
    both((lang) => {
      for (const field of ['name', 'description', 'message', 'reason', 'fingerprint', 'findingId']) {
        const said = errorMessage(err(400, 'field_too_long', `${field} is longer than 5 characters`, { field, max: 5, actual: 6 }))
        expect(said, `${lang} ${field}`).not.toContain('longer than') // not the sentence of the server
        expect(said, `${lang} ${field}`).not.toContain(`“${field}”`) // and not the key of the field in place of its label
        if (lang === 'ru') expect(said).toMatch(/[А-Яа-яЁё]{4}/)
      }
      expect(errorMessage(err(400, 'field_too_long', 'x', { field: 'notes', max: 5, actual: 6 }))).toContain('notes')
    })
  })

  it('a project name outside its bounds says how long it may be', () => {
    setLang('ru')
    expect(errorMessage(err(400, 'invalid_name', 'project name must be 1–200 characters', { field: 'name', max: 200, actual: 0 }))).toBe('Название: от 1 до 200 знаков, сейчас 0')
    setLang('en')
    expect(errorMessage(err(400, 'invalid_name', 'project name must be 1–200 characters', { field: 'name', max: 200, actual: 0 }))).toBe('The name must be 1 to 200 characters, now 0')
  })

  it('an answer that carries no numbers is shown by the sentence of the server, not guessed at', () => {
    expect(errorMessage(err(400, 'field_too_long', 'description is longer than 5 characters'))).toBe('description is longer than 5 characters')
    expect(errorMessage(err(400, 'field_too_long', 'description is longer than 5 characters', { field: 'description' }))).toBe('description is longer than 5 characters')
  })
})

describe('too many changes, a request that took too long, a body that is too large', () => {
  it('are said in each language, with the numbers', () => {
    setLang('ru')
    expect(errorMessage(err(400, 'ops_too_many', 'a request takes at most 50 operations', { max: 50, actual: 51 }))).toBe('За один запрос — не больше 50 правок, а их 51')
    expect(errorMessage(err(504, 'request_timeout', 'the request took longer than 60 seconds (limits.requestTimeoutSeconds) and was stopped'))).toContain('Сервер не успел ответить')
    expect(errorMessage(err(413, 'payload_too_large', 'the request body is larger than the limit (limits.maxBodyMb)'))).toContain('больше, чем принимает сервер')
    setLang('en')
    expect(errorMessage(err(400, 'ops_too_many', 'a request takes at most 50 operations', { max: 50, actual: 51 }))).toBe('One request takes at most 50 changes, and there are 51')
    expect(errorMessage(err(504, 'request_timeout', 'the request took longer than 60 seconds'))).toContain('did not answer in time')
    expect(errorMessage(err(413, 'payload_too_large', 'x'))).toContain('larger than the server accepts')
  })
})

describe('a busy server', () => {
  const user = (retryAfter?: number) => err(429, 'rate_limited', 'you already have 3 heavy requests running', { scope: 'user', limit: 3 }, retryAfter)
  const replica = (retryAfter?: number) => err(429, 'rate_limited', 'this server is busy with 16 heavy requests', { scope: 'replica', limit: 16 }, retryAfter)

  it('says how long to wait, and for the limit of one person, what the limit is', () => {
    setLang('ru')
    expect(errorMessage(user(2))).toBe('У вас уже идёт столько расчётов, сколько сервер ведёт для одного человека (3). Подождите, пока один закончится: около 2 с')
    expect(errorMessage(replica(2))).toBe('Сервер занят другими расчётами. Повторите через 2 с')
    setLang('en')
    expect(errorMessage(user(2))).toBe('You already have as many calculations running as the server runs for one person (3). Wait for one to finish: about 2 s')
    expect(errorMessage(replica(2))).toBe('The server is busy with other calculations. Try again in 2 s')
  })

  it('without a time to wait says only that it is busy', () => {
    both(() => {
      expect(errorMessage(replica())).toBe(errorMessage(user()))
      expect(errorMessage(replica())).not.toContain('heavy requests')
    })
    setLang('en')
    expect(errorMessage(replica())).toBe('The server is busy with other calculations. Try again in a moment')
  })

  it('is only the heavy-request gate: the sign-in limit has the same code and its own sentence', () => {
    const login = err(429, 'rate_limited', 'too many sign-in attempts, try again later', undefined, 30)
    expect(serverRefusal(login)).toBeNull()
    expect(errorMessage(login)).toBe('too many sign-in attempts, try again later')
  })
})

describe('a setting value that was not accepted', () => {
  const refused = (key: string, reasonCode: string, params: Record<string, unknown> | null, reason: string) =>
    err(400, 'invalid_setting', `${key}: ${reason}`, { key, reason, reasonCode, params })

  it('is said in the language of the page, with the key in front (the toast) and without it (under the field)', () => {
    const e = refused('ui.helpUrl', 'not_http', null, 'must be an address that starts with http:// or https://')
    expect(settingRefusal(e)).toEqual({ key: 'ui.helpUrl', message: 'Нужен адрес, который начинается с http:// или https://' })
    expect(errorMessage(e)).toBe('ui.helpUrl: Нужен адрес, который начинается с http:// или https://')
    setLang('en')
    expect(settingRefusal(e)).toEqual({ key: 'ui.helpUrl', message: 'An address that starts with http:// or https:// is required' })
    expect(errorMessage(e)).toBe('ui.helpUrl: An address that starts with http:// or https:// is required')
  })

  it('takes the numbers and the options from `params`, with the words of the form', () => {
    setLang('ru')
    expect(settingRefusal(refused('auth.sessionTtlHours', 'below_min', { min: 1 }, 'must be ≥ 1'))?.message).toBe('Не меньше 1')
    expect(settingRefusal(refused('limits.maxBodyMb', 'above_max', { max: 256 }, 'must be ≤ 256'))?.message).toBe('Не больше 256')
    expect(settingRefusal(refused('ui.theme', 'not_an_option', { options: 'auto, light, dark' }, 'must be one of: auto, light, dark'))?.message).toBe('Допустимые значения: auto, light, dark')
    expect(settingRefusal(refused('app.name', 'line_too_long', { max: 200 }, 'must be at most 200 characters'))?.message).toBe('Не длиннее 200 знаков')
    setLang('en')
    expect(settingRefusal(refused('auth.sessionTtlHours', 'below_min', { min: 1 }, 'must be ≥ 1'))?.message).toBe('Must be at least 1')
    expect(settingRefusal(refused('app.name', 'line_too_long', { max: 200 }, 'must be at most 200 characters'))?.message).toBe('At most 200 characters')
  })

  it('names the entry of a list that is wrong', () => {
    const e = refused('versions.sourceBases', 'not_http', { value: 'ftp://mirror.internal/' }, "'ftp://mirror.internal/' must be an address that starts with http:// or https://")
    expect(settingRefusal(e)?.message).toBe('«ftp://mirror.internal/»: Нужен адрес, который начинается с http:// или https://')
    setLang('en')
    expect(settingRefusal(e)?.message).toBe('“ftp://mirror.internal/”: An address that starts with http:// or https:// is required')
  })

  it('the types are the words of the form', () => {
    setLang('ru')
    for (const [expected, said] of [['string', 'Нужна строка'], ['number', 'Нужно число'], ['boolean', 'Нужно значение «да» или «нет»'], ['list', 'Нужен список строк'], ['object', 'Нужен JSON-объект или массив']] as const) {
      expect(settingRefusal(refused('k.k', 'wrong_type', { expected }, 'expected a thing'))?.message).toBe(said)
    }
  })

  it('a code the page does not know yet is shown by the English sentence, never by nothing', () => {
    both(() => {
      expect(settingRefusal(refused('k.k', 'a_brand_new_reason', { n: 1 }, 'must be cooler'))?.message).toBe('must be cooler')
      expect(settingRefusal(refused('k.k', 'invalid', null, 'unknown format zzz'))?.message).toBe('unknown format zzz')
    })
  })

  it('an answer without the key (an older server) is not read as a setting refusal', () => {
    expect(settingRefusal(err(400, 'invalid_setting', 'ui.helpUrl: must be an address'))).toBeNull()
    expect(errorMessage(err(400, 'invalid_setting', 'ui.helpUrl: must be an address'))).toBe('ui.helpUrl: must be an address')
    expect(settingRefusal(new Error('boom'))).toBeNull()
    expect(settingRefusal(err(400, 'conflict', 'x', { key: 'a' }))).toBeNull()
  })
})

describe('everything else is as it was', () => {
  it('another code keeps the sentence of the server, whatever its details', () => {
    expect(errorMessage(err(409, 'conflict', 'the project was changed by someone else', { currentRev: 7 }))).toBe('the project was changed by someone else')
    expect(errorMessage(err(500, 'internal', 'internal error'))).toBe('internal error')
    expect(serverRefusal(err(404, 'not_found', 'no such project'))).toBeNull()
    expect(serverRefusal(new Error('boom'))).toBeNull()
    expect(serverRefusal(undefined)).toBeNull()
  })
})

// ---------------------------------------------------------------------------------------------------------------------------------------------------------------------
// The two ends of the contract: what the Rust server can say, and what the page can say about it. A new code or a new capped field on the server that nobody gave words
// to fails here, not in front of an administrator who reads another language.
// ---------------------------------------------------------------------------------------------------------------------------------------------------------------------

function rustSources(dir: string): string[] {
  return readdirSync(dir).flatMap((f) => {
    const p = join(dir, f)
    return statSync(p).isDirectory() ? rustSources(p) : p.endsWith('.rs') ? [p] : []
  })
}
const serverSrc = join(process.cwd(), '..', 'backend', 'crates', 'server', 'src')
const rust = rustSources(serverSrc).map((p) => readFileSync(p, 'utf8')).join('\n')
const unique = (re: RegExp) => [...new Set([...rust.matchAll(re)].map((m) => m[1]))].sort()

describe('the server and the page agree on the words', () => {
  // `invalid` is the code of a refusal nobody gave a code of its own (a registry that is broken, not a person who is wrong): it is shown by its sentence
  const answeredWithTheForm = ['below_min', 'above_max', 'not_an_option', 'wrong_type']

  it('has a text, in both languages, for every reason a setting value can be refused for', () => {
    const codes = unique(/Refusal::new\(\s*"([a-z_]+)"/g).filter((c) => c !== 'invalid')
    expect(codes.length, 'the codes were found in the sources of the server').toBeGreaterThan(10)
    const missing: string[] = []
    for (const lang of ['ru', 'en']) {
      setLang(lang)
      for (const code of codes) if (!answeredWithTheForm.includes(code) && !has(`error.setting.${code}`)) missing.push(`${lang}: error.setting.${code}`)
    }
    expect(missing).toEqual([])
  })

  it('knows what each of the wrong types is', () => {
    const kinds = unique(/expected\(\s*"([a-z]+)"/g)
    expect(kinds.sort()).toEqual(['boolean', 'list', 'number', 'object', 'string'])
    for (const expected of kinds) {
      const english = 'expected a thing'
      expect(settingRefusal(err(400, 'invalid_setting', 'k.k: x', { key: 'k.k', reason: english, reasonCode: 'wrong_type', params: { expected } }))?.message).not.toBe(english)
    }
  })

  it('has a label for every field the server caps', () => {
    const fields = unique(/TextField::\w+,\s*"(\w+)"/g)
    expect(fields).toEqual(expect.arrayContaining(['name', 'description', 'message', 'reason', 'fingerprint', 'findingId']))
    expect(fields.filter((f) => !has(`error.field.${f}`))).toEqual([])
  })

  it('has a text for every refusal the editor can meet on a gated route', () => {
    // the codes of the budgets (`field_too_long`, `invalid_name`, `ops_too_many`, `request_timeout`, `payload_too_large`, and `rate_limited` of the gate) are the ones the page words
    for (const code of ['field_too_long', 'invalid_name', 'ops_too_many', 'request_timeout', 'payload_too_large']) expect(rust, code).toContain(`"${code}"`)
    for (const k of ['error.fieldTooLong', 'error.invalidName', 'error.opsTooMany', 'error.requestTimeout', 'error.payloadTooLarge', 'error.busy', 'error.busy.soon', 'error.busy.user']) {
      for (const lang of ['ru', 'en']) {
        setLang(lang)
        expect(has(k), `${lang}: ${k}`).toBe(true)
      }
    }
  })
})
