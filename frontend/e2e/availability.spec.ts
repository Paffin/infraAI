import { expect, type Page } from '@playwright/test'
import { test } from './fixtures'
import { createProject, login, openAnalysis, uniqueName } from './helpers'

const TEMPLATE = '2 ЦОД + witness'

const slo = (page: Page) => page.locator('.right [data-req="availabilitySlo"] input')

test.describe('expected availability: how many nines the design gives and what eats the budget', () => {
  test.beforeEach(async ({ page }) => {
    await login(page)
  })

  test('the objective tile shows the estimate against the target and opens the explanation', async ({ page }) => {
    await createProject(page, uniqueName('avail'), TEMPLATE)
    const tile = page.locator('.objectives .obj', { hasText: 'Доступность' })
    await expect(tile).toBeVisible({ timeout: 15_000 })
    // the template carries a 99.95 % target and the estimate is a number with its nines, not a raw fraction
    await expect(tile).toContainText('99.95%')
    await expect(tile).toContainText(/≈ 99\.\d+%/)
    await expect(tile).not.toContainText('0.99')
    await tile.click()
    const hero = page.getByTestId('avail-hero')
    await expect(hero).toBeVisible()
    await expect(hero).toContainText(/99\.\d+%/)
    await expect(hero).toContainText('цель 99.95%')
    // what the downtime consists of, which services cost most, which failures: all from the engine's report
    await expect(page.locator('.av .causes [data-cause]').first()).toBeVisible()
    expect(await page.locator('.av .causes [data-cause]').count()).toBeGreaterThanOrEqual(3)
    expect(await page.locator('.av .units .line').count()).toBeGreaterThanOrEqual(2)
    expect(await page.locator('.av .top .fail').count()).toBeGreaterThanOrEqual(3)
  })

  test('picking a target sets the requirement and the verdict follows it, in the panel and in the findings', async ({ page }) => {
    await createProject(page, uniqueName('avail-target'), TEMPLATE)
    await openAnalysis(page, 'Доступность')
    const hero = page.getByTestId('avail-hero')
    await expect(hero).toBeVisible({ timeout: 15_000 })
    await expect(page.locator('.av .verdict').first()).toContainText('Укладывается')

    // four nines are more than this design gives: the verdict, the tile and a finding say so
    await page.locator('.av .chip.pr', { hasText: '99.99%' }).click()
    await expect(page.locator('.av .chip.pr.on')).toContainText('99.99%')
    await expect(page.locator('.av .verdict').first()).toContainText('Не укладывается', { timeout: 15_000 })
    await expect(hero).toContainText('цель 99.99%')
    await page.locator('.right .tab', { hasText: 'Проверки' }).click()
    const finding = page.locator('.finding', { hasText: 'AV-SLO-01' })
    await expect(finding).toBeVisible({ timeout: 15_000 })
    await expect(finding).toContainText('при цели 99.99%')
    await expect(page.locator('.objectives .obj.critical', { hasText: 'Доступность' })).toBeVisible()

    // the requirement is the same field the form shows
    await page.locator('.right .tab', { hasText: 'Требования' }).click()
    await expect(slo(page)).toHaveValue('99.99')
    await expect(page.locator('.right [data-req="availabilitySlo"] .hint').first()).toContainText('до')

    // and a looser target takes the finding away
    await openAnalysis(page, 'Доступность')
    await page.locator('.av .chip.pr', { hasText: '99.9%' }).first().click()
    await expect(page.locator('.av .verdict').first()).toContainText('Укладывается', { timeout: 15_000 })
    await page.locator('.right .tab', { hasText: 'Проверки' }).click()
    await expect(page.locator('.finding', { hasText: 'AV-SLO-01' })).toHaveCount(0)
  })

  test('without a target the estimate is still there and asks for one', async ({ page }) => {
    await createProject(page, uniqueName('avail-none'), TEMPLATE)
    await page.locator('.right .tab', { hasText: 'Требования' }).click()
    await slo(page).fill('')
    await openAnalysis(page, 'Доступность')
    await expect(page.getByTestId('avail-hero')).toContainText('цель не задана', { timeout: 15_000 })
    await expect(page.locator('.av .verdict')).toContainText('Цель не задана')
    // the finding asks for the field, like the RTO and the budget do
    await page.locator('.right .tab', { hasText: 'Проверки' }).click()
    const need = page.locator('.finding', { hasText: 'AV-SLO-01' }).locator('.need')
    await expect(need).toBeVisible({ timeout: 15_000 })
    await need.click()
    await expect(slo(page)).toBeFocused()
  })

  test('the costliest failure can be replayed on the diagram and a service shown on it', async ({ page }) => {
    await createProject(page, uniqueName('avail-replay'), TEMPLATE)
    await openAnalysis(page, 'Доступность')
    await expect(page.locator('.av .top .fail').first()).toBeVisible({ timeout: 15_000 })
    await expect(page.locator('.whatif .res')).toHaveCount(0)
    await page.locator('.av .top .fail').first().getByRole('button', { name: 'Показать отказ на схеме' }).click()
    await expect(page.locator('.whatif .res')).toBeVisible({ timeout: 15_000 })
    await page.locator('.av .units .name').first().click()
    await expect(page.locator('.arch-node.lit').first()).toBeVisible()
  })

  test('the assumptions are listed and an administrator is sent to the settings', async ({ page }) => {
    await createProject(page, uniqueName('avail-basis'), TEMPLATE)
    await openAnalysis(page, 'Доступность')
    const basis = page.locator('.av .assume')
    await expect(basis).toBeVisible({ timeout: 15_000 })
    await basis.locator('summary').click()
    await expect(basis).toContainText('Отказ экземпляра')
    await expect(basis).toContainText('Управляемые сервисы')
    await expect(basis.getByRole('button', { name: 'Открыть настройки движка' })).toBeVisible()
  })
})
