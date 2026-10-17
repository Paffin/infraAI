import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { expect, type Page } from '@playwright/test'
import type { Architecture, RecommendPlan } from '../src/api/types'
import { test } from './fixtures'
import { login, projectWith, uniqueName } from './helpers'

// A server that is busy says so: `429 rate_limited` with `Retry-After` (the calculations one person has running at once are limited, `limits.heavyConcurrentPerUser`).
// The editor asks for the verdict, the failure being watched and the plan of improvements by itself after every edit, so for it that answer is "later", not an error:
// it keeps showing that it is calculating, asks again when the server says to, and shows the result. A request the person made (a version, an export) is answered in
// words they read.
//
// Nothing here is a double. The gate is the real one and is occupied by a real, long calculation: a design of ten sites copied side by side, evaluated again and again
// through the same session (`hold`), so the editor's own request meets a slot that is really taken. The test does not time anything by eye: it holds until it has
// seen the server refuse the editor (`429` on the page's own request, with the `Retry-After` the server sent) and only then lets go.

const recorded = JSON.parse(readFileSync(join(process.cwd(), 'tests', 'fixtures', 'plan-spof.json'), 'utf8')) as { architecture: Architecture; plan: RecommendPlan }

/** `copies` of a design side by side, every id made new (zones, cards, links, and what a prop or a group says about them): one evaluation of it takes the server seconds on a debug build. */
function tiled(arch: Architecture, copies: number): Architecture {
  const out: Architecture = { ...arch, zones: [], nodes: [], edges: [] }
  for (let i = 0; i < copies; i++) {
    const ids = new Map<string, string>([...arch.zones, ...arch.nodes].map((x) => [x.id, `${x.id}-t${i}`]))
    const renamed = (v: string | null) => (v == null ? v : (ids.get(v) ?? v))
    const props = (p: Record<string, unknown>) => Object.fromEntries(Object.entries(p).map(([k, v]) => [k, typeof v === 'string' ? (ids.get(v) ?? v) : v]))
    for (const z of arch.zones) out.zones.push({ ...z, id: ids.get(z.id)!, parentId: renamed(z.parentId), props: props(z.props) as typeof z.props })
    for (const n of arch.nodes) out.nodes.push({ ...n, id: ids.get(n.id)!, zoneId: renamed(n.zoneId), group: n.group == null ? n.group : `${n.group}-t${i}`, props: props(n.props) as typeof n.props })
    for (const e of arch.edges) out.edges.push({ ...e, id: `${e.id}-t${i}`, source: renamed(e.source)!, target: renamed(e.target)!, props: props(e.props) as typeof e.props })
  }
  return out
}

// ten sites: 51 cards, 11 zones, 91 links; nine copies stay under the bounds of a design (500 cards, 100 zones)
const COPIES = 9

async function bigDesign(page: Page): Promise<Architecture> {
  const tpl = await page.request.get('/api/templates/ten-sites-regions')
  expect(tpl.ok(), 'the template of ten sites').toBe(true)
  const big = tiled(((await tpl.json()) as { architecture: Architecture }).architecture, COPIES)
  expect(big.nodes.length).toBeGreaterThan(400)
  return big
}

/** Evaluates `design` again and again through the session of the page, so that the slot of the user stays taken. Stops when told (the one that is running ends first). */
function hold(page: Page, design: Architecture) {
  let on = true
  const loop = (async () => {
    try {
      while (on) {
        const r = await page.request.post('/api/evaluate', { data: { architecture: design }, timeout: 120_000 })
        if (r.status() === 429) await new Promise((done) => setTimeout(done, 150)) // the editor got the slot for a moment: this goes again
      }
    } catch { /* the page is gone: nothing left to hold */ }
  })()
  return {
    async stop() {
      on = false
      await loop
    },
  }
}

/** The slot is taken: a heavy request with nothing in it is refused at the door (the gate comes before the body is read), and is let in, and told it is nonsense, when the slot is free. */
async function untilHeld(page: Page) {
  await expect.poll(async () => (await page.request.post('/api/evaluate', { data: {} })).status(), { timeout: 60_000, intervals: [100], message: 'the slot of the user is taken' }).toBe(429)
}

interface Seen { status: number; at: number; retryAfter: string | null }
/** What the server answered to the page's own requests to `path`, with the time and the `Retry-After` it sent. */
function watch(page: Page, path: string): Seen[] {
  const seen: Seen[] = []
  page.on('response', (res) => {
    if (res.request().method() === 'POST' && new URL(res.url()).pathname === path) seen.push({ status: res.status(), at: Date.now(), retryAfter: res.headers()['retry-after'] ?? null })
  })
  return seen
}
const refused = (seen: Seen[]) => seen.filter((s) => s.status === 429)

/** The calculations of one person are limited to one at a time, a refusal says "again in a second", and the failure being played has no lead. The settings are put back. */
async function withLimits(page: Page, body: () => Promise<void>) {
  const keys = ['limits.heavyConcurrentPerUser', 'limits.heavyRetryAfterSeconds', 'editor.failureLeadMs']
  const all = ((await (await page.request.get('/api/admin/settings')).json()) as { settings: { key: string; value: unknown }[] }).settings
  const was = Object.fromEntries(keys.map((k) => [k, all.find((s) => s.key === k)!.value]))
  const put = (changes: Record<string, unknown>) => page.request.put('/api/admin/settings', { data: { changes } })
  expect((await put({ 'limits.heavyConcurrentPerUser': 1, 'limits.heavyRetryAfterSeconds': 1, 'editor.failureLeadMs': 0 })).ok()).toBe(true)
  try {
    await body()
  } finally {
    await put(was)
  }
}

test.describe('a busy server is "later", not an error', () => {
  test.beforeEach(async ({ page }) => {
    await login(page)
  })

  test('the verdict is asked for again after the Retry-After the server sent, and comes; no error is shown meanwhile', async ({ page }) => {
    test.setTimeout(180_000)
    await withLimits(page, async () => {
      const id = await projectWith(page, uniqueName('busy'), recorded.architecture)
      const big = await bigDesign(page)
      const evaluations = watch(page, '/api/evaluate')
      const holding = hold(page, big)
      try {
        await untilHeld(page)
        await page.goto(`/p/${id}`)
        // the server refuses the editor's own calculation, twice, a second apart (the Retry-After it sent); the editor says it is still calculating and shows no error
        await expect.poll(() => refused(evaluations).length, { timeout: 60_000, message: 'the server refused the first calculation of the editor, and the second' }).toBeGreaterThanOrEqual(2)
        const [first, second] = refused(evaluations)
        expect([first.retryAfter, second.retryAfter]).toEqual(['1', '1'])
        expect(second.at - first.at, 'asked again after the second the server asked to wait, not at once and not never').toBeGreaterThanOrEqual(800)
        expect(second.at - first.at).toBeLessThan(5_000)
        await expect(page.locator('.sk-list[aria-busy="true"]')).toBeVisible()
        await expect(page.locator('.banner-err')).toHaveCount(0)
        await expect(page.locator('.toast.error')).toHaveCount(0)
        await expect(page.locator('.tiles .tile')).toHaveCount(0)
      } finally {
        await holding.stop()
      }
      // the slot is free: the next ask is answered, and the verdict is on the screen
      await expect(page.locator('.tiles .tile').first()).toBeVisible({ timeout: 30_000 })
      await expect(page.locator('.sk-list[aria-busy="true"]')).toHaveCount(0)
      await expect(page.locator('.banner-err')).toHaveCount(0)
      expect(evaluations.at(-1)?.status).toBe(200)
    })
  })

  test('a failure is asked for again, and played when the server has room; the plan of improvements waits as well', async ({ page }) => {
    test.setTimeout(240_000)
    await withLimits(page, async () => {
      const id = await projectWith(page, uniqueName('busy-sim'), recorded.architecture)
      const big = await bigDesign(page)
      const simulations = watch(page, '/api/simulate')
      const plans = watch(page, '/api/recommendations')
      const evaluations = watch(page, '/api/evaluate')
      // the verdict comes (the server is free) and the slot is taken the moment it does, before the editor asks for the plan of improvements (`editor.recommendMs` after the verdict)
      let holding: ReturnType<typeof hold> | undefined
      page.on('response', (res) => {
        if (!holding && res.request().method() === 'POST' && new URL(res.url()).pathname === '/api/evaluate' && res.ok()) holding = hold(page, big)
      })
      await page.goto(`/p/${id}`)
      await expect(page.locator('.arch-node').first()).toBeVisible()
      await expect.poll(() => evaluations.some((s) => s.status === 200), { timeout: 30_000 }).toBe(true)
      try {
        await expect.poll(() => refused(plans).length, { timeout: 60_000, message: 'the server refused the plan of improvements' }).toBeGreaterThanOrEqual(2)
        expect(refused(plans)[0].retryAfter).toBe('1')
        // no card with a sentence of the server in it: the plan is simply not here yet
        await expect(page.getByTestId('recommendations')).toHaveCount(0)

        // a failure chosen while the slot is taken: the card waits (a spinner, no error), and the server is asked again
        await page.locator('.whatif button', { hasText: 'ЦОД-1' }).first().click()
        await expect.poll(() => refused(simulations).length, { timeout: 60_000, message: 'the server refused the failure' }).toBeGreaterThanOrEqual(2)
        expect(refused(simulations)[0].retryAfter).toBe('1')
        await expect(page.locator('[data-testid="simcard"].waiting')).toBeVisible()
        await expect(page.locator('.banner-err')).toHaveCount(0)
        await expect(page.locator('.whatif .c.crit')).toHaveCount(0)
      } finally {
        await holding?.stop()
      }
      // room again: the failure is played, and the plan of improvements arrives
      await expect(page.locator('[data-testid="sim-tiles"]')).toBeVisible({ timeout: 30_000 })
      await expect(page.locator('[data-testid="simcard"].waiting')).toHaveCount(0)
      // the plan is in the Checks tab, which a failure on screen takes the place of: leave the failure
      await page.locator('.whatif .x').click()
      await expect(page.getByTestId('recommendations').locator('[data-rec]')).toHaveCount(recorded.plan.items.length, { timeout: 30_000 })
      await expect(page.getByTestId('recommendations').locator('.err')).toHaveCount(0)
      await expect(page.locator('.banner-err')).toHaveCount(0)
    })
  })

  test('a project closed while a calculation waits is not asked about, and nothing is left "calculating"', async ({ page }) => {
    test.setTimeout(180_000)
    await withLimits(page, async () => {
      const id = await projectWith(page, uniqueName('busy-leave'), recorded.architecture)
      const big = await bigDesign(page)
      const evaluations = watch(page, '/api/evaluate')
      const holding = hold(page, big)
      try {
        await untilHeld(page)
        await page.goto(`/p/${id}`)
        await expect.poll(() => refused(evaluations).length, { timeout: 60_000 }).toBeGreaterThanOrEqual(1)
        await page.getByRole('link', { name: 'Проекты' }).click()
        await expect(page.locator('article.project').first()).toBeVisible()
        const asked = evaluations.length
        // two Retry-After periods pass in the list of projects: the editor that was waiting is gone, and does not ask
        await page.waitForTimeout(2_600)
        expect(evaluations.length, 'no request after the editor was left').toBe(asked)
      } finally {
        await holding.stop()
      }
    })
  })

  test('a request the person made is answered in words they read: the server is busy, and for how long', async ({ page }) => {
    test.setTimeout(180_000)
    await withLimits(page, async () => {
      const id = await projectWith(page, uniqueName('busy-version'), recorded.architecture)
      const big = await bigDesign(page)
      await page.goto(`/p/${id}`)
      await expect(page.locator('.tiles .tile').first()).toBeVisible({ timeout: 30_000 })
      const holding = hold(page, big)
      try {
        await untilHeld(page)
        await page.locator('button[title="Версии · diff"]').click()
        await page.locator('.vers .save input').fill('saved while the server is busy')
        await page.locator('.vers .save .btn-primary').click()
        const toast = page.locator('.toast.error').first()
        // one calculation at a time is the limit of this person here (1), the sentence is in Russian, and it is not the English one of the server
        await expect(toast).toContainText('У вас уже идёт столько расчётов, сколько сервер ведёт для одного человека (1)')
        await expect(toast).toContainText('около 1 с')
        await expect(toast).not.toContainText('heavy requests')
      } finally {
        await holding.stop()
      }
    })
  })
})
