import { expect } from '@playwright/test'
import { test } from './fixtures'
import { createProject, login, uniqueName } from './helpers'

// a newcomer: this spec does not switch the tours off (every other spec does, see fixtures.ts)
test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => { try { localStorage.removeItem('ia_tour_off') } catch { /* ignore */ } })
})

const tour = (page: import('@playwright/test').Page) => page.locator('[data-testid="tour"]')

test.describe('the guided tour', () => {
  test('welcomes a newcomer on the project list, points at real elements, and does not come back', async ({ page }) => {
    await login(page)
    await expect(tour(page)).toBeVisible({ timeout: 15_000 })
    await expect(tour(page).locator('.count')).toContainText('1')
    await expect(tour(page).locator('h3')).toHaveText('Новый проект')
    // the hole is around the button the step is about
    const box = await page.locator('[data-tour="new-project"]').boundingBox()
    const hole = await tour(page).locator('.hole').boundingBox()
    expect(hole && box && hole.x <= box.x && hole.y <= box.y && hole.x + hole.width >= box.x + box.width).toBeTruthy()

    await page.keyboard.press('ArrowRight')
    await expect(tour(page).locator('h3')).toHaveText('Ваши проекты')
    await tour(page).getByTestId('tour-prev').click()
    await expect(tour(page).locator('h3')).toHaveText('Новый проект')
    // through to the end
    for (let i = 0; i < 4; i++) await tour(page).getByTestId('tour-next').click()
    await expect(tour(page)).toHaveCount(0)

    await page.reload()
    await expect(page.locator('.brand')).toBeVisible()
    await page.waitForTimeout(1200)
    await expect(tour(page)).toHaveCount(0)
  })

  test('Esc skips it, the Help menu starts it again, and "do not show again" is remembered for the editor too', async ({ page }) => {
    await login(page)
    await expect(tour(page)).toBeVisible({ timeout: 15_000 })
    await page.keyboard.press('Escape')
    await expect(tour(page)).toHaveCount(0)

    await page.locator('.help-menu .trigger').click()
    await page.getByTestId('help-tour').click()
    await expect(tour(page)).toBeVisible()
    await tour(page).getByTestId('tour-never').click()
    await expect(tour(page)).toHaveCount(0)
    expect(await page.evaluate(() => localStorage.getItem('ia_tour_off'))).toBe('1')

    // the editor does not greet this user either
    await createProject(page, uniqueName('tourless'), '2 ЦОД + witness')
    await page.waitForTimeout(1500)
    await expect(tour(page)).toHaveCount(0)
    // but the menu still has the tour of this screen
    await page.locator('.help-menu .trigger').click()
    await page.getByTestId('help-tour').click()
    await expect(tour(page).locator('h3')).toHaveText('Схема — это модель')
  })

  test('the editor tour opens the library, points at the findings and ends with a word', async ({ page }) => {
    await login(page)
    await expect(tour(page)).toBeVisible({ timeout: 15_000 })
    await page.keyboard.press('Escape')
    await createProject(page, uniqueName('tourful'), '2 ЦОД + witness')
    await expect(tour(page)).toBeVisible({ timeout: 15_000 })
    // the opening word has nothing to point at
    await expect(tour(page).locator('.hole')).toHaveCount(0)
    await expect(tour(page).locator('.card')).toHaveClass(/centered/)

    await tour(page).getByTestId('tour-next').click()
    await expect(tour(page).locator('h3')).toHaveText('Библиотека')
    await expect(tour(page).locator('.hole')).toBeVisible()
    await expect(page.locator('[data-tour="library"]')).toBeVisible()

    // every step that points at something points at something that is there; the closing word ends it
    const seen: string[] = ['Библиотека']
    for (let i = 0; i < 12; i++) {
      const last = (await tour(page).getByTestId('tour-next').innerText()) === 'Готово'
      if (last) break
      const before = seen[seen.length - 1]
      await tour(page).getByTestId('tour-next').click()
      await expect(tour(page).locator('h3')).not.toHaveText(before)
      seen.push(await tour(page).locator('h3').innerText())
    }
    // the stack has no key for the AI, so there is no prompt bar to point at, and the tour does not stop at it (a step whose element is not on the screen is not shown)
    expect(seen).toEqual(['Библиотека', 'Оценка и достоверность', 'Замечания', 'Что если отказ', 'Экспорт', 'Всё под рукой', 'Готово'])
    await tour(page).getByTestId('tour-next').click()
    await expect(tour(page)).toHaveCount(0)
  })

  test('the administrator can turn the tours off for everybody', async ({ page }) => {
    await login(page)
    await page.keyboard.press('Escape')
    const put = (v: boolean) => page.request.put('/api/admin/settings', { data: { changes: { 'ui.onboarding': v } } })
    expect((await put(false)).ok()).toBeTruthy()
    try {
      // a newcomer: another user, nothing remembered
      await page.evaluate(() => localStorage.clear())
      await page.reload()
      await expect(page.locator('.brand')).toBeVisible()
      await page.waitForTimeout(1500)
      await expect(tour(page)).toHaveCount(0)
      // the Help menu still offers it
      await page.locator('.help-menu .trigger').click()
      await expect(page.getByTestId('help-tour')).toBeVisible()
    } finally {
      await put(true)
    }
  })
})
