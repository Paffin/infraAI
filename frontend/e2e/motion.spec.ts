import { expect } from '@playwright/test'
import { test } from './fixtures'
import { createProject, login, uniqueName } from './helpers'

const mode = (page: import('@playwright/test').Page) => page.evaluate(() => document.documentElement.dataset.motion)

test.describe('motion: full, reduced, off', () => {
  test('follows the system setting until the user chooses', async ({ browser }) => {
    const calm = await browser.newContext({ reducedMotion: 'reduce', locale: 'ru-RU' })
    const page = await calm.newPage()
    await login(page)
    expect(await mode(page)).toBe('reduced')
    // nothing travels in the reduced mode, but things still fade
    expect(await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--move').trim())).toBe('0px')
    await calm.close()

    const lively = await browser.newContext({ reducedMotion: 'no-preference', locale: 'ru-RU' })
    const p2 = await lively.newPage()
    await login(p2)
    expect(await mode(p2)).toBe('full')
    expect(await p2.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--move').trim())).toBe('8px')
    await lively.close()
  })

  test('the choice is made in the menu, applies at once and is kept', async ({ page }) => {
    await login(page)
    // the top bar is drawn again while the page finishes loading: open the menu until it stays open
    await expect(page.getByRole('button', { name: 'Новый проект' }).first()).toBeVisible()
    await expect(async () => {
      await page.locator('.motion-switch .trigger').click()
      await expect(page.locator('.motion-switch .menu')).toBeVisible({ timeout: 1500 })
    }).toPass({ timeout: 10_000 })
    await page.locator('.motion-switch [data-pref="off"]').click()
    expect(await mode(page)).toBe('off')
    // off: buttons change at once
    const ms = await page.locator('.btn').first().evaluate((el) => parseFloat(getComputedStyle(el).transitionDuration.split(',')[0]) * 1000)
    expect(ms).toBeLessThan(5)
    await page.reload()
    await expect(page.locator('.brand')).toBeVisible()
    expect(await mode(page)).toBe('off')
    await page.locator('.motion-switch .trigger').click()
    await expect(page.locator('.motion-switch [data-pref="off"]')).toHaveClass(/cur/)
    await page.locator('.motion-switch [data-pref="full"]').click()
    expect(await mode(page)).toBe('full')
    // back to what the administrator decided
    await page.evaluate(() => localStorage.removeItem('ia_motion'))
  })

  test('the administrator sets the default and the speed; a user who has not chosen follows them', async ({ page }) => {
    await login(page)
    const put = (changes: Record<string, unknown>) => page.request.put('/api/admin/settings', { data: { changes } })
    expect((await put({ 'ui.motion': 'reduced', 'ui.motionScale': 200 })).ok()).toBeTruthy()
    try {
      await page.evaluate(() => localStorage.removeItem('ia_motion'))
      await page.reload()
      await expect(page.locator('.brand')).toBeVisible()
      expect(await mode(page)).toBe('reduced')
      const scale = await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--motion-scale').trim())
      expect(scale).toBe('2')
      // the durations double with it
      const fast = await page.evaluate(() => { const d = document.createElement('div'); d.style.transitionDuration = 'var(--dur-fast)'; document.body.appendChild(d); const v = getComputedStyle(d).transitionDuration; d.remove(); return v })
      expect(parseFloat(fast) * 1000).toBeCloseTo(280, 0)
    } finally {
      await put({ 'ui.motion': 'system', 'ui.motionScale': 100 })
    }
  })

  test('a dialog fades out before it goes, and goes at once without animations', async ({ page }) => {
    await login(page)
    await createProject(page, uniqueName('motion'), '2 ЦОД + witness')
    await page.locator('.canvas').click({ position: { x: 5, y: 400 } })
    await page.keyboard.press('Shift+?')
    const modal = page.locator('.modal-backdrop')
    await expect(modal).toBeVisible()
    expect(await modal.evaluate((el) => getComputedStyle(el).animationName)).toContain('backdrop-in')
    await page.keyboard.press('Escape')
    await expect(modal).toHaveClass(/closing/)
    await expect(modal).toHaveCount(0)

    await page.locator('.motion-switch .trigger').click()
    await page.locator('.motion-switch [data-pref="off"]').click()
    await page.locator('.canvas').click({ position: { x: 5, y: 400 } })
    await page.keyboard.press('Shift+?')
    await expect(modal).toBeVisible()
    await page.keyboard.press('Escape')
    await expect(modal).toHaveCount(0, { timeout: 500 })
    await page.evaluate(() => localStorage.removeItem('ia_motion'))
  })

  test('the tab underline slides to the tab you pick and the panel changes with a transition', async ({ page }) => {
    await login(page)
    await createProject(page, uniqueName('tabs'), '2 ЦОД + witness')
    const ink = page.locator('.right .ink')
    const x = () => ink.evaluate((el) => (el as HTMLElement).style.transform)
    const before = await x()
    await page.locator('.right .tab', { hasText: 'Анализ' }).click()
    await page.locator('.right .seg .segbtn', { hasText: 'Стоимость' }).click()
    await expect.poll(x).not.toBe(before)
    await expect(page.locator('.right .cost')).toBeVisible()
    await page.locator('.right .tab', { hasText: 'Проверки' }).click()
    await expect(page.locator('.right .checks')).toBeVisible()
    await expect.poll(x).toBe(before)
  })
})
