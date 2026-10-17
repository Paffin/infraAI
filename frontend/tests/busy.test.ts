import { createServer, type Server } from 'node:http'
import type { AddressInfo } from 'node:net'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { ApiError, http } from '../src/api/client'
import { setLang } from '../src/i18n'
import { busyRetryMs } from '../src/lib/busy'
import { useApp } from '../src/stores/app'
import { useEditor } from '../src/stores/editor'
import spof from './fixtures/report-spof.json'

// A busy server answers `429 rate_limited` with `Retry-After` (docs/API.md; the Rust side is `heavy_endpoints_are_rate_limited`). The server here is a real HTTP
// server on a port of its own that says exactly that, and the real `fetch` of the page talks to it: what is read from the answer is read from a real response.
// The pure part is `busyRetryMs`; the part that matters is that the editor store does not show "busy" as an error and asks again.

type Reply = { status: number; headers?: Record<string, string>; body: unknown }
let server: Server
let hits: string[] = []
let reply: (n: number, url: string) => Reply
const realFetch = globalThis.fetch

beforeEach(async () => {
  hits = []
  server = createServer((req, res) => {
    hits.push(req.url ?? '')
    req.resume()
    const r = reply(hits.length, req.url ?? '')
    res.writeHead(r.status, { 'Content-Type': 'application/json', ...r.headers })
    res.end(JSON.stringify(r.body))
  })
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  const base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`
  // the page asks for '/api/…' of its own origin; there is none here, so the address of this server is put in front
  globalThis.fetch = ((input: RequestInfo | URL, init?: RequestInit) => realFetch(typeof input === 'string' && input.startsWith('/') ? base + input : input, init)) as typeof fetch
  setActivePinia(createPinia())
  setLang('ru')
})

afterEach(async () => {
  globalThis.fetch = realFetch
  await new Promise<void>((resolve) => server.close(() => resolve()))
})

const busy = (retryAfter?: string): Reply => ({
  status: 429,
  headers: retryAfter ? { 'Retry-After': retryAfter } : {},
  body: { error: { code: 'rate_limited', message: 'you already have 3 heavy requests running', details: { scope: 'user', limit: 3 } } },
})

const failure = async (p: Promise<unknown>) => p.then(() => { throw new Error('the request was expected to fail') }, (e) => e)

describe('Retry-After', () => {
  it('is read into the error, in seconds', async () => {
    reply = () => busy('3')
    const e = await failure(http.post('/evaluate', {}))
    expect(e).toBeInstanceOf(ApiError)
    expect([e.status, e.code, e.retryAfter]).toEqual([429, 'rate_limited', 3])
    expect(busyRetryMs(e)).toBe(3000)
  })

  it('is not there when the server did not say it, and only a busy server is "later"', async () => {
    reply = () => busy()
    expect(busyRetryMs(await failure(http.post('/evaluate', {})))).toBeNull() // a refusal that names no time is shown, not retried
    reply = () => ({ status: 429, headers: { 'Retry-After': '30' }, body: { error: { code: 'locked', message: 'locked' } } })
    expect(busyRetryMs(await failure(http.post('/auth/login', {})))).toBeNull() // another 429
    reply = () => ({ status: 503, headers: { 'Retry-After': '5' }, body: { error: { code: 'unavailable', message: 'draining' } } })
    expect(busyRetryMs(await failure(http.get('/projects')))).toBeNull() // another status
    expect(busyRetryMs(new ApiError(0, 'network', 'offline'))).toBeNull()
    expect(busyRetryMs(new Error('boom'))).toBeNull()
    expect(busyRetryMs(undefined)).toBeNull()
  })
})

describe('the editor and a busy server', () => {
  function open() {
    const app = useApp()
    app.user = { id: 'u1', email: 'a@b.c', name: 'A', role: 'editor', mustChangePassword: false }
    app.boot = { editor: {} } as never
    const editor = useEditor()
    editor.projectId = 'p1'
    return editor
  }
  const until = async (done: () => boolean, ms: number) => {
    const end = Date.now() + ms
    while (!done() && Date.now() < end) await new Promise((r) => setTimeout(r, 20))
  }

  it('asks for the verdict again when told to, and shows no error meanwhile', async () => {
    reply = (n) => (n === 1 ? busy('1') : { status: 200, body: spof.report })
    const editor = open()
    await editor.evaluateNow()
    expect(editor.evalError).toBe('')
    expect(editor.evaluating).toBe(true) // still being calculated: the retry is pending
    expect(editor.report).toBeNull()
    await until(() => editor.report !== null, 4000)
    expect(hits).toEqual(['/api/evaluate', '/api/evaluate'])
    expect(editor.report?.score.value).toBe(spof.report.score.value)
    expect(editor.evalError).toBe('')
    expect(editor.evaluating).toBe(false)
    editor.reset()
  })

  it('shows every other failure as before, and does not ask again', async () => {
    reply = () => ({ status: 500, body: { error: { code: 'internal', message: 'internal error' } } })
    const editor = open()
    await editor.evaluateNow()
    expect(editor.evalError).toBe('internal error')
    expect(editor.evaluating).toBe(false)
    await new Promise((r) => setTimeout(r, 1300))
    expect(hits).toHaveLength(1)
    editor.reset()
  })

  it('a project closed while the retry is pending is not asked about, and is not left "calculating"', async () => {
    reply = () => busy('1')
    const editor = open()
    await editor.evaluateNow()
    expect(editor.evaluating).toBe(true)
    editor.reset()
    expect(editor.evaluating).toBe(false)
    await new Promise((r) => setTimeout(r, 1300))
    expect(hits).toHaveLength(1)
  })

  it('asks for a failure scenario again, and drops the retry when another scenario is chosen', async () => {
    const result = { state: 'up', nodeStates: {}, events: [], traffic: { served: 1, lost: {}, flows: {} }, trafficBefore: { served: 1, flows: {} } }
    reply = (n) => (n === 1 ? busy('1') : { status: 200, body: result })
    const editor = open()
    await editor.runScenario({ type: 'zone', zoneId: 'dc1' })
    expect(editor.sim?.loading).toBe(true)
    expect(editor.sim?.error).toBe('')
    await until(() => editor.sim?.loading === false, 4000)
    expect(hits).toEqual(['/api/simulate', '/api/simulate'])
    expect(editor.sim?.error).toBe('')
    expect(editor.sim?.result).toBeTruthy()

    hits = []
    reply = () => busy('1')
    await editor.runScenario({ type: 'zone', zoneId: 'dc1' })
    editor.exitSim() // the scenario is left before the retry
    await new Promise((r) => setTimeout(r, 1300))
    expect(hits).toHaveLength(1)
    editor.reset()
  })
})
