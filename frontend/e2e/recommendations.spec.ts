import { expect, type Page } from '@playwright/test'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { test } from './fixtures'
import { login, projectWith, uniqueName } from './helpers'
import type { Architecture, RecommendPlan } from '../src/api/types'

// The design the engine's own plan was recorded for (tests/fixtures/plan-spof.json): two data centers and a witness, GSLB and WAF as one instance each.
const recorded = JSON.parse(readFileSync(join(process.cwd(), 'tests', 'fixtures', 'plan-spof.json'), 'utf8')) as { architecture: Architecture; plan: RecommendPlan }
const FIRST = recorded.plan.items[0].id
const SECOND = recorded.plan.items[1].id

const card = (page: Page) => page.getByTestId('recommendations')
const item = (page: Page, id: string) => card(page).locator(`[data-rec="${id}"]`)
/** The number of critical findings on the tile (the element always holds the final value, not the middle of the count). */
const criticals = async (page: Page) => Number(await page.locator('.tiles .tile.crit .anum').getAttribute('data-value'))

async function open(page: Page) {
  const id = await projectWith(page, uniqueName('rec'), recorded.architecture)
  await page.goto(`/p/${id}`)
  await expect(page.locator('.arch-node').first()).toBeVisible()
  return id
}

test.describe('what to improve first: the engine tries its own fixes and ranks them by what they measurably do', () => {
  test.beforeEach(async ({ page }) => {
    await login(page)
  })

  test('the checks list the fixes in order, each with the effect the engine measured, and what all of them do together', async ({ page }) => {
    await open(page)
    await expect(card(page)).toBeVisible({ timeout: 20_000 })
    await expect(card(page).locator('[data-rec]')).toHaveCount(recorded.plan.items.length)
    // the order is the engine's: the fix that closes the most comes first
    await expect(card(page).locator('[data-rec]').first()).toHaveAttribute('data-rec', FIRST)
    await expect(card(page).locator('[data-rec]').nth(1)).toHaveAttribute('data-rec', SECOND)

    const first = item(page, FIRST)
    await expect(first).toContainText('HA-SPOF-01')
    await expect(first.locator('[data-fx="critical"]')).toHaveText(/критичных \d+ → \d+/)
    await expect(first.locator('[data-fx="critical"]')).toHaveAttribute('title', /Закрывает: .*HA-SPOF-01/)
    await expect(first.locator('[data-fx="score"]')).toHaveText(/оценка \d+ → \d+/)
    // the availability reads with its nines, never as a raw fraction, and the tooltip says what it means in hours
    const avail = first.locator('[data-fx="availability"]')
    await expect(avail).toHaveText(/доступность 99\.\d+% → 99\.\d+%/)
    await expect(avail).toHaveAttribute('title', /Простой в год: .+ → .+/)
    await expect(first.locator('[data-fx="cost"]')).toHaveText(/^\+.+₽ в мес$/)

    // everything at once: the figures of the whole bundle, not the sum of the single ones
    const all = page.getByTestId('rec-all')
    await expect(all).toBeVisible()
    await expect(all.locator('[data-fx="critical"]')).toHaveText(/критичных \d+ → 0/)
    await expect(all.getByRole('button', { name: /Применить всё \(2\)/ })).toBeEnabled()
  })

  test('applying one changes the design, the plan is made again for the new one, and the toast takes it back', async ({ page }) => {
    await open(page)
    await expect(item(page, FIRST)).toBeVisible({ timeout: 20_000 })
    const before = await criticals(page)
    expect(before).toBeGreaterThan(0)

    await item(page, FIRST).getByRole('button', { name: /^Применить/ }).click()
    await expect(page.locator('.toast', { hasText: 'Применено:' })).toBeVisible()
    await expect.poll(() => criticals(page), { timeout: 15_000 }).toBeLessThan(before)
    // what is done is not offered again; the next step stays
    await expect(item(page, FIRST)).toHaveCount(0, { timeout: 20_000 })
    await expect(item(page, SECOND)).toBeVisible()

    // the way back
    await page.locator('.toast .act', { hasText: 'Отменить' }).click()
    await expect.poll(() => criticals(page), { timeout: 15_000 }).toBe(before)
    await expect(item(page, FIRST)).toBeVisible({ timeout: 20_000 })
  })

  test('"apply all" takes the design to where the bundle said, and with nothing left to improve the card is gone', async ({ page }) => {
    await open(page)
    const all = page.getByTestId('rec-all')
    await expect(all).toBeVisible({ timeout: 20_000 })
    await all.getByRole('button', { name: /Применить всё/ }).click()
    await expect(page.locator('.toast', { hasText: 'Применено шагов: 2' })).toBeVisible()
    await expect.poll(() => criticals(page), { timeout: 15_000 }).toBe(0)
    await expect(card(page)).toHaveCount(0, { timeout: 20_000 })
    // one undo takes back all of it
    await page.locator('.toast .act', { hasText: 'Отменить' }).click()
    await expect.poll(() => criticals(page), { timeout: 15_000 }).toBeGreaterThan(0)
  })

  test('the number listed and the "show more" fold are settings of the engine', async ({ page }) => {
    const current = async () => (await (await page.request.get('/api/admin/config/engine')).json()).items[0].data
    const put = (data: unknown) => page.request.put('/api/admin/config/engine/engine', { data: { data } })
    const settings = await current()
    try {
      expect((await put({ ...settings, recommendations: { ...settings.recommendations, show: 1 } })).ok()).toBeTruthy()
      await open(page)
      await expect(card(page).locator('[data-rec]')).toHaveCount(1, { timeout: 20_000 })
      await card(page).getByRole('button', { name: 'Показать ещё 1' }).click()
      await expect(card(page).locator('[data-rec]')).toHaveCount(2)
      await card(page).getByRole('button', { name: 'Свернуть' }).click()
      await expect(card(page).locator('[data-rec]')).toHaveCount(1)

      // switched off: the checks are as before and the server does not do the work
      expect((await put({ ...settings, recommendations: { ...settings.recommendations, enabled: false } })).ok()).toBeTruthy()
      await page.reload()
      await expect(page.locator('.finding').first()).toBeVisible({ timeout: 20_000 })
      await expect(card(page)).toHaveCount(0)
      const r = await page.request.post('/api/recommendations', { data: { architecture: recorded.architecture } })
      expect((await r.json()).items).toEqual([])
    } finally {
      await page.request.post('/api/admin/config/engine/engine/reset')
    }
    await page.reload()
    await expect(card(page).locator('[data-rec]').first()).toBeVisible({ timeout: 20_000 })
  })

  test('moving a card on the canvas does not make the plan wait: the design is the same', async ({ page }) => {
    await open(page)
    await expect(item(page, FIRST)).toBeVisible({ timeout: 20_000 })
    // a drag changes where the card lies, not the design: the plan stays usable (not "waiting")
    const node = page.locator('.arch-node').first()
    const box = (await node.boundingBox())!
    await page.mouse.move(box.x + box.width / 2, box.y + 14)
    await page.mouse.down()
    await page.mouse.move(box.x + box.width / 2 + 40, box.y + 34, { steps: 6 })
    await page.mouse.up()
    await page.waitForTimeout(1600) // the check and the pause before a new plan have both run
    await expect(card(page)).not.toHaveClass(/waiting/)
    await expect(item(page, FIRST).getByRole('button', { name: /^Применить/ })).toBeEnabled()
  })
})
