import { expect, type Page } from '@playwright/test'
import { test } from './fixtures'
import { createProject, login, uniqueName } from './helpers'

/** The rectangles of everything that floats over the canvas, and of every card in it. */
async function geometry(page: Page) {
  return page.evaluate(() => {
    const rect = (el: Element) => { const r = el.getBoundingClientRect(); return { l: r.left, t: r.top, r: r.right, b: r.bottom } }
    const host = rect(document.querySelector('.canvas')!)
    return {
      host,
      overlays: Array.from(document.querySelectorAll('[data-overlay]')).filter((e) => e.getBoundingClientRect().height > 0).map(rect),
      cards: Array.from(document.querySelectorAll('.arch-node')).map(rect),
    }
  })
}

/** A card is "behind a panel" when more than a sliver of it lies under one. */
const hidden = (g: Awaited<ReturnType<typeof geometry>>) =>
  g.cards.filter((c) => g.overlays.some((o) => Math.min(c.r, o.r) - Math.max(c.l, o.l) > 2 && Math.min(c.b, o.b) - Math.max(c.t, o.t) > 2)).length

const zoom = async (page: Page) => parseInt((await page.locator('.zoom .pct').innerText()).replace('%', ''), 10)

/** The zoom once it has stopped changing (a zoom button glides to its value). */
async function settledZoom(page: Page) {
  let last = -1
  for (let i = 0; i < 20; i++) {
    const now = await zoom(page)
    if (now === last) return now
    last = now
    await page.waitForTimeout(300)
  }
  throw new Error('the zoom never settled')
}

test.describe('the canvas keeps the diagram clear of the panels over it', () => {
  test.beforeEach(async ({ page }) => {
    await login(page)
  })

  test('a diagram that fills the screen is fit into what the toolbar and the what-if bar leave free', async ({ page }) => {
    await createProject(page, uniqueName('fit'), '10 площадок')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    // the what-if bar grows a row once the scenarios are known; the diagram follows it, so wait for the picture to settle
    await expect.poll(async () => hidden(await geometry(page)), { timeout: 15_000, message: 'no card is behind a panel' }).toBe(0)
    const g = await geometry(page)
    expect(g.cards.length).toBeGreaterThan(10)
    // and all of them are inside the canvas, not cut off by its edge
    expect(g.cards.every((c) => c.l >= g.host.l - 1 && c.r <= g.host.r + 1 && c.t >= g.host.t - 1 && c.b <= g.host.b + 1)).toBe(true)
  })

  test('"Fit" brings the diagram back from wherever it was taken, clear of the panels', async ({ page }) => {
    await createProject(page, uniqueName('refit'), '3 AZ в одном регионе')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await page.locator('.zoom button[aria-label="+"]').click()
    await page.locator('.zoom button[aria-label="+"]').click()
    await page.locator('.zoom .fit').click()
    await expect.poll(async () => hidden(await geometry(page)), { timeout: 10_000 }).toBe(0)
  })

  test('a view the user has chosen is not moved for them when a card is added', async ({ page }) => {
    await createProject(page, uniqueName('own'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await page.locator('.zoom button[aria-label="−"]').click()
    await page.locator('.zoom button[aria-label="−"]').click()
    const chosen = await settledZoom(page)
    expect(chosen).toBeLessThan(60)
    const before = await page.locator('.arch-node').count()
    await page.locator('.library .kind', { hasText: 'Redis' }).first().click()
    await expect(page.locator('.arch-node')).toHaveCount(before + 1)
    await page.waitForTimeout(900)
    expect(await zoom(page)).toBe(chosen)
  })

  test('while the diagram is still at its fit, a card that is added is brought into view', async ({ page }) => {
    await createProject(page, uniqueName('grow'))
    for (const kind of ['Kubernetes', 'PostgreSQL', 'Redis', 'Kafka', 'RabbitMQ', 'Vault']) {
      await page.locator('.library .kind', { hasText: kind }).first().click()
      await page.waitForTimeout(150)
    }
    await expect(page.locator('.arch-node')).toHaveCount(6)
    await expect.poll(async () => hidden(await geometry(page)), { timeout: 10_000 }).toBe(0)
    const g = await geometry(page)
    expect(g.cards.every((c) => c.l >= g.host.l - 1 && c.r <= g.host.r + 1 && c.t >= g.host.t - 1 && c.b <= g.host.b + 1)).toBe(true)
  })
})
