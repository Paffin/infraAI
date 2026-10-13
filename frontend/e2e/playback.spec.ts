import { expect } from '@playwright/test'
import { test } from './fixtures'
import { createProject, login, uniqueName } from './helpers'

test.describe('playback of a failure', () => {
  test.beforeEach(async ({ page }) => {
    await login(page)
    await createProject(page, uniqueName('play'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
  })

  const replica = (page: import('@playwright/test').Page) => page.locator('.arch-node', { hasText: 'PostgreSQL replica' })

  test('the card tells the recovery in steps; playing shows the service before and after each of them', async ({ page }) => {
    await page.locator('.whatif button', { hasText: 'ЦОД-1' }).click()
    const card = page.locator('[data-testid="simcard"]')
    await expect(card).toBeVisible({ timeout: 15_000 })
    const steps = card.locator('.steps > li')
    expect(await steps.count()).toBeGreaterThanOrEqual(3)

    // a failure is shown settled: the last step, the replica is the writer
    await expect(card.locator('.steps > li.cur')).toHaveAttribute('data-step', String((await steps.count()) - 1))
    await expect(replica(page)).toHaveClass(/sim-promoted/)

    // the moment of the failure: the service is down and nobody is the writer yet
    await steps.first().click()
    await expect(card.locator('[data-testid="sim-state"]')).toHaveClass(/down/)
    await expect(replica(page)).not.toHaveClass(/sim-promoted/)
    await expect(card.locator('[data-testid="clock"]')).toHaveText('отказ')

    // play it (fast): it ends where the settled result is
    await card.locator('.sp', { hasText: '×4' }).click()
    await card.locator('[data-testid="play"]').click()
    await expect(card.locator('.steps > li.cur')).toHaveAttribute('data-step', String((await steps.count()) - 1), { timeout: 15_000 })
    await expect(replica(page)).toHaveClass(/sim-promoted/)
    await expect(card.locator('[data-testid="clock"]')).toContainText('+')

    // the user takes over: a step back is one step back, and pausing stays paused
    await card.getByRole('button', { name: 'Шаг назад' }).click()
    await expect(card.locator('.steps > li.cur')).toHaveAttribute('data-step', String((await steps.count()) - 2))
    await page.waitForTimeout(600)
    await expect(card.locator('.steps > li.cur')).toHaveAttribute('data-step', String((await steps.count()) - 2))
  })

  test('without animations the steps still turn, and the track is keyboard-operable', async ({ page }) => {
    await page.locator('[data-motion-menu]').click()
    await page.locator('.motion-switch [data-pref="off"]').click()
    await page.locator('.whatif button', { hasText: 'ЦОД-1' }).click()
    const card = page.locator('[data-testid="simcard"]')
    await expect(card).toBeVisible({ timeout: 15_000 })
    const track = card.locator('.track')
    await track.focus()
    await page.keyboard.press('Home')
    await expect(card.locator('.steps > li.cur')).toHaveAttribute('data-step', '0')
    await expect(replica(page)).not.toHaveClass(/sim-promoted/)
    await page.keyboard.press('End')
    await expect(replica(page)).toHaveClass(/sim-promoted/)
    await page.evaluate(() => localStorage.removeItem('ia_motion'))
  })

  test('leaving the simulation clears the picture', async ({ page }) => {
    await page.locator('.whatif button', { hasText: 'ЦОД-1' }).click()
    const card = page.locator('[data-testid="simcard"]')
    await expect(card).toBeVisible({ timeout: 15_000 })
    await card.getByRole('button', { name: 'Выйти из симуляции' }).click()
    await expect(card).toHaveCount(0)
    await expect(page.locator('.arch-node[class*="sim-"]')).toHaveCount(0)
  })
})
