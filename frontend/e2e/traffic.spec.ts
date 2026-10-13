import { expect, type Page } from '@playwright/test'
import { test } from './fixtures'
import { createProject, login, uniqueName } from './helpers'

/** The administrator's settings that decide how a failure is played; a test that changes them puts them back. */
const put = (page: Page, changes: Record<string, number>) => page.request.put('/api/admin/settings', { data: { changes } })
const DEFAULTS = { 'editor.failureLeadMs': 2400, 'editor.playbackStepMs': 1600 }

/** A card that is not as it is in normal operation while a failure is on screen. */
const BROKEN = '.arch-node:is(.sim-down, .sim-stalled, .sim-partial, .sim-fenced, .sim-isolated, .sim-promoted)'
const served = (page: Page) => page.getByTestId('simcard').locator('[data-testid="traffic-served"] .anum')

test.describe('the traffic in a failure: how it goes without it, and what becomes of it', () => {
  test.beforeEach(async ({ page }) => {
    await login(page)
    expect((await put(page, { 'editor.failureLeadMs': 0 })).ok()).toBeTruthy()
  })
  test.afterEach(async ({ page }) => {
    await put(page, DEFAULTS)
  })

  test('the story starts with normal operation, where every link carries its share and requests are served in full', async ({ page }) => {
    await createProject(page, uniqueName('traffic'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    // no failure: no particles, no failure tab
    await expect(page.locator('.particles')).toHaveCount(0)
    await page.locator('.whatif button', { hasText: 'ЦОД-1' }).click()
    const card = page.getByTestId('simcard')
    await expect(card).toBeVisible({ timeout: 15_000 })
    // running a failure opens its tab
    await expect(page.locator('.right .tab.active')).toHaveText(/Отказ/)

    await card.locator('.steps > li').first().click()
    await expect(served(page)).toHaveAttribute('data-value', '100')
    await expect(page.getByTestId('story-caption')).toContainText('Так идёт трафик без отказа')
    await expect(page.getByTestId('story-caption')).toContainText('100%')
    // the requests are on their way along many links, nothing is lost, no card is down
    await expect.poll(() => page.locator('g.traffic.carrying').count()).toBeGreaterThan(8)
    await expect(page.locator('g.traffic.dropping')).toHaveCount(0)
    await expect.poll(() => page.locator('.particles circle').count()).toBeGreaterThan(8)
    await expect(page.locator('.particles .pl')).toHaveCount(0)
    await expect(page.locator(BROKEN)).toHaveCount(0)
    await expect(card.getByTestId('traffic-lost')).toHaveCount(0)
    // the load of the peak is the declared one
    await expect(card).toContainText('5 000 из 5 000 запросов в секунду')
  })

  test('at the moment of the failure the load goes to the surviving site, and what cannot get through is dropped where it cannot', async ({ page }) => {
    await createProject(page, uniqueName('traffic-fail'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await page.locator('.whatif button', { hasText: 'ЦОД-1' }).click()
    const card = page.getByTestId('simcard')
    await expect(card).toBeVisible({ timeout: 15_000 })
    await card.locator('.steps > li').nth(1).click()

    // the database has no writer yet: every request fails
    await expect(served(page)).toHaveAttribute('data-value', '0')
    await expect(page.getByTestId('story-caption')).toContainText('Отказ')
    await expect(card.getByTestId('traffic-lost').first()).toContainText('PostgreSQL')
    // the cache is a soft dependency: its calls go unanswered but the requests go on
    await expect(card.getByTestId('traffic-missed').first()).toContainText('Redis')
    // red particles die on the link, and the link says what it loses
    await expect.poll(() => page.locator('.particles .pl').count()).toBeGreaterThan(0)
    await expect(page.locator('.edge-label.flow.lost').first()).toContainText('теряется')
    await expect(page.locator('.edge-label.flow.missed').first()).toContainText('нет ответа')
    // the surviving site takes twice what it took, and the cards say so
    const doubled = page.locator('.arch-node .load-tag.up', { hasText: '×2' })
    await expect(doubled.first()).toBeVisible()
    await expect(page.locator('.edge-label.flow', { hasText: '2 500 rps → 5 000 rps' }).first()).toBeVisible()
    // nothing arrives at the lost site, and the cards that are down do not say "—" on top of "отказ"
    await expect(page.locator('.arch-node.sim-down .load-tag')).toHaveCount(0)

    // the replica is promoted: the requests are served again, up to what the site can bear
    await card.locator('.steps > li').last().click()
    await expect(served(page)).toHaveAttribute('data-value', '96')
    await expect(page.locator('.edge-label.flow.lost', { hasText: 'PostgreSQL' })).toHaveCount(0)
  })

  test('a balancer keeps sending to the lost member until it notices, and then the traffic goes around, along a route that is not drawn', async ({ page }) => {
    await createProject(page, uniqueName('traffic-detour'), '3 AZ в одном регионе')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await page.locator('.whatif button', { hasText: 'Узел' }).click()
    // the application of the second zone (the cards are in the order of their zones)
    await page.locator('.arch-node', { hasText: 'Kubernetes' }).nth(1).click()
    const card = page.getByTestId('simcard')
    await expect(card).toBeVisible({ timeout: 15_000 })
    const steps = card.locator('.steps > li')
    expect(await steps.count()).toBe(3)

    // until the health checks notice, a third of the requests still go to the lost application and fail
    await steps.nth(1).click()
    await expect(served(page)).toHaveAttribute('data-value', '67')
    await expect(card.getByTestId('traffic-lost').first()).toBeVisible()
    // (the brokers share their load between them whoever sends it, so some detours are there already: those of the balancer are not)
    await expect.poll(() => page.locator('g.detour').count()).toBeGreaterThan(0)
    const early = await page.locator('g.detour').count()

    // then they go to the applications that are left, along links that are not on the diagram
    await steps.nth(2).click()
    await expect(served(page)).toHaveAttribute('data-value', '100')
    await expect(card.getByTestId('traffic-detour').first()).toBeVisible()
    await expect.poll(() => page.locator('g.detour').count()).toBeGreaterThanOrEqual(early + 2)
    await expect(page.locator('.edge-label.flow.detour-label').first()).toContainText('в обход')
    await expect(card.getByTestId('traffic-lost')).toHaveCount(0)
    // and the legend of the card names it
    await expect(card.locator('.legend .detour')).toBeVisible()

    // going back to normal operation takes the detours away again
    await steps.nth(0).click()
    await expect(page.locator('g.detour')).toHaveCount(0)
    await expect(page.locator('.edge-label.flow.detour-label')).toHaveCount(0)
  })

  test('a failure that has just been run tells its story by itself: normal operation, the failure, the outcome', async ({ page }) => {
    expect((await put(page, { 'editor.failureLeadMs': 800, 'editor.playbackStepMs': 400 })).ok()).toBeTruthy()
    await createProject(page, uniqueName('traffic-auto'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await page.locator('.whatif button', { hasText: 'ЦОД-1' }).click()
    const caption = page.getByTestId('story-caption')
    await expect(caption).toContainText('Так идёт трафик без отказа', { timeout: 15_000 })
    await expect(caption).toContainText('Отказ: ЦОД-1', { timeout: 15_000 })
    // it ends where the settled result is, and the player on the canvas says it can play again
    const steps = page.getByTestId('simcard').locator('.steps > li')
    const last = String((await steps.count()) - 1)
    await expect(page.getByTestId('simcard').locator('.steps > li.cur')).toHaveAttribute('data-step', last, { timeout: 20_000 })
    await expect(page.getByTestId('caption-play')).toHaveAttribute('aria-label', 'Воспроизвести')
    // ▶ plays it again, from normal operation
    await page.getByTestId('caption-play').click()
    await expect(caption).toContainText('Так идёт трафик без отказа')
  })

  test('the controls over the canvas step through the story like those of the card', async ({ page }) => {
    await createProject(page, uniqueName('traffic-ctl'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await page.locator('.whatif button', { hasText: 'ЦОД-1' }).click()
    const card = page.getByTestId('simcard')
    await expect(card).toBeVisible({ timeout: 15_000 })
    const caption = page.getByTestId('story-caption')
    await expect(caption.getByRole('button', { name: 'Шаг вперёд' })).toBeDisabled()
    await caption.getByRole('button', { name: 'Шаг назад' }).click()
    await caption.getByRole('button', { name: 'Шаг назад' }).click()
    await caption.getByRole('button', { name: 'Шаг назад' }).click()
    await expect(card.locator('.steps > li.cur')).toHaveAttribute('data-step', '0')
    await expect(caption.getByRole('button', { name: 'Шаг назад' })).toBeDisabled()
    await caption.getByRole('button', { name: 'Шаг вперёд' }).click()
    await expect(caption).toContainText('Отказ: ЦОД-1')
  })

  test('closing the failure takes its tab and its picture away and goes back to the tab that was open', async ({ page }) => {
    await createProject(page, uniqueName('traffic-exit'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await page.locator('.right .tab', { hasText: 'Требования' }).click()
    await page.locator('.whatif button', { hasText: 'ЦОД-1' }).click()
    await expect(page.getByTestId('simcard')).toBeVisible({ timeout: 15_000 })
    await expect(page.locator('.right .tab', { hasText: 'Отказ' })).toBeVisible()
    await page.getByTestId('simcard').getByRole('button', { name: 'Выйти из симуляции' }).click()
    await expect(page.locator('.right .tab', { hasText: 'Отказ' })).toHaveCount(0)
    await expect(page.locator('.right .tab.active')).toHaveText(/Требования/)
    await expect(page.getByTestId('story-caption')).toHaveCount(0)
    await expect(page.locator('.particles')).toHaveCount(0)
    await expect(page.locator('g.detour')).toHaveCount(0)
  })

  test('without animations there are no particles, and the widths, colours and numbers still tell the traffic', async ({ page }) => {
    await page.evaluate(() => { try { localStorage.setItem('ia_motion', 'off') } catch { /* the default motion applies */ } })
    await page.reload()
    await expect(page.locator('html')).toHaveAttribute('data-motion', 'off')
    await createProject(page, uniqueName('traffic-still'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await page.locator('.whatif button', { hasText: 'ЦОД-1' }).click()
    const card = page.getByTestId('simcard')
    await expect(card).toBeVisible({ timeout: 15_000 })
    await card.locator('.steps > li').nth(1).click()
    await expect(served(page)).toHaveAttribute('data-value', '0')
    await expect(page.locator('.particles')).toHaveCount(0)
    await expect.poll(() => page.locator('g.traffic.carrying').count()).toBeGreaterThan(3)
    await expect(page.locator('.edge-label.flow.lost').first()).toBeVisible()
  })

  test('the particles are the administrator\'s: none at 0, and the count is capped', async ({ page }) => {
    await createProject(page, uniqueName('traffic-cap'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    try {
      expect((await put(page, { 'editor.flowParticlesMax': 0 })).ok()).toBeTruthy()
      await page.reload()
      await expect(page.locator('.arch-node').first()).toBeVisible()
      await page.locator('.whatif button', { hasText: 'ЦОД-1' }).click()
      await expect(page.getByTestId('simcard')).toBeVisible({ timeout: 15_000 })
      await page.getByTestId('simcard').locator('.steps > li').first().click()
      await expect.poll(() => page.locator('g.traffic.carrying').count()).toBeGreaterThan(3)
      await expect(page.locator('.particles')).toHaveCount(0)

      expect((await put(page, { 'editor.flowParticlesMax': 12 })).ok()).toBeTruthy()
      await page.reload()
      await expect(page.locator('.arch-node').first()).toBeVisible()
      await page.locator('.whatif button', { hasText: 'ЦОД-1' }).click()
      await expect(page.getByTestId('simcard')).toBeVisible({ timeout: 15_000 })
      await page.getByTestId('simcard').locator('.steps > li').first().click()
      await expect.poll(() => page.locator('.particles circle').count()).toBeGreaterThan(0)
      // thinned out: far fewer than the default would put on this diagram, and still every link that carries something has one
      expect(await page.locator('.particles circle').count()).toBeLessThan(40)
    } finally {
      await put(page, { 'editor.flowParticlesMax': 480 })
    }
  })
})
