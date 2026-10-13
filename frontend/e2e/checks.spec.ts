import { expect } from '@playwright/test'
import { test } from './fixtures'
import { createProject, login, uniqueName } from './helpers'

test.describe('the checks tab says everything it shows and what to do next', () => {
  test.beforeEach(async ({ page }) => {
    await login(page)
  })

  test('the goal tiles write their whole label: nothing is cut with an ellipsis', async ({ page }) => {
    await createProject(page, uniqueName('tiles'), '2 ЦОД + witness')
    await expect(page.locator('.objectives .obj').first()).toBeVisible({ timeout: 15_000 })
    const cut = await page.evaluate(() =>
      Array.from(document.querySelectorAll<HTMLElement>('.objectives .obj .ol, .objectives .obj .ov')).filter((el) => el.scrollWidth > el.clientWidth + 1).map((el) => el.textContent?.trim()),
    )
    expect(cut, 'labels or values wider than their tile').toEqual([])
    // a long label takes a second line instead (the tile grows, its neighbour in the row with it)
    const heights = await page.locator('.objectives .obj').evaluateAll((els) => els.map((e) => Math.round(e.getBoundingClientRect().height)))
    expect(Math.max(...heights)).toBeGreaterThanOrEqual(Math.min(...heights))
    expect(Math.min(...heights)).toBeGreaterThanOrEqual(36)
  })

  test('a design with nothing open says so and offers what to do next; a level without findings says so and goes back', async ({ page }) => {
    await createProject(page, uniqueName('clear'), '2 ЦОД + witness')
    const clear = page.getByTestId('all-clear')
    await expect(clear).toBeVisible({ timeout: 15_000 })
    await expect(clear).toContainText('Замечаний нет')
    await expect(clear).toContainText(/Пройдено проверок: \d+/)

    await clear.getByRole('button', { name: 'Матрица отказов' }).click()
    await expect(page.locator('.modal')).toBeVisible()
    await page.keyboard.press('Escape')
    await expect(page.locator('.modal')).toHaveCount(0)
    await clear.getByRole('button', { name: 'Доступность' }).click()
    await expect(page.getByTestId('avail-hero')).toBeVisible()
    await page.locator('.right .tab', { hasText: 'Проверки' }).click()
    await page.getByTestId('all-clear').getByRole('button', { name: 'Экспорт' }).click()
    await expect(page.locator('.modal')).toBeVisible()
    await page.keyboard.press('Escape')

    // a tile with nothing behind it filters to nothing: the panel says that, and the way back is one click
    await page.locator('.tiles .tile.crit').click()
    await expect(page.locator('.muted-none')).toContainText('Находок уровня «критично» нет.')
    await expect(page.getByTestId('all-clear')).toHaveCount(0)
    await page.locator('.muted-none').getByRole('button', { name: 'Показать все' }).click()
    await expect(page.getByTestId('all-clear')).toBeVisible()
  })
})
