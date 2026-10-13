import { expect, test, type Page } from '@playwright/test'
import { login, uniqueName } from './helpers'

/** Opens the placement wizard from the new-project dialog and waits for the first preview. */
async function openWizard(page: Page) {
  await page.getByRole('button', { name: 'Новый проект' }).first().click()
  await page.locator('.wizard-card').click()
  await expect(page.locator('.wiz .site').first()).toBeVisible({ timeout: 20_000 })
}

const pattern = (page: Page, text: string) => page.locator('.wiz .pat', { hasText: text })
const sites = (page: Page, n: number) => page.locator('.wiz .quick .chip', { hasText: new RegExp(`^${n}$`) }).click()

test.describe('placement: from one data center to ten sites', () => {
  test.beforeEach(async ({ page }) => {
    await login(page)
  })

  test('five sites: the wizard shows what is survived and the project opens with it', async ({ page }) => {
    await openWizard(page)
    await sites(page, 5)
    // the engine's verdict: two sites at once, nothing critical
    await expect(page.locator('.wiz .surv', { hasText: '2 площадок сразу' })).toBeVisible({ timeout: 20_000 })
    await expect(page.locator('.wiz .site')).toHaveCount(5)
    await expect(page.locator('.wiz .ok-line')).toBeVisible()
    // the comparison answers "what if there were another number of sites"
    await expect(page.locator('.wiz .cmp tbody tr').first()).toBeVisible({ timeout: 20_000 })
    expect(await page.locator('.wiz .cmp tbody tr').count()).toBeGreaterThanOrEqual(4)

    const name = uniqueName('five')
    await page.locator('.wiz .cfg input.input').first().fill(name)
    await page.locator('.modal-foot .btn-primary').click()
    await page.waitForSelector('.canvas')
    await expect(page.locator('.objectives .obj', { hasText: '2 площадки сразу' })).toBeVisible({ timeout: 20_000 })
    // the site picker of the what-if bar lists them
    await page.locator('.whatif button', { hasText: 'Несколько сразу' }).click()
    await page.locator('.whatif .menu .chk').nth(0).check()
    await page.locator('.whatif .menu .chk').nth(1).check()
    await page.locator('.whatif .menu .go').click()
    await expect(page.locator('.whatif .res')).toBeVisible({ timeout: 15_000 })
    await expect(page.locator('.whatif .res')).not.toHaveClass(/crit/)
  })

  test('one data center: racks are the failure domains', async ({ page }) => {
    await openWizard(page)
    await pattern(page, 'Один ЦОД').click()
    await expect(page.locator('.wiz .surv', { hasText: 'отказ стойки' })).toBeVisible({ timeout: 20_000 })
    await expect(page.locator('.wiz .rack').first()).toBeVisible()
    await page.locator('.wiz .cfg input.input').first().fill(uniqueName('onedc'))
    await page.locator('.modal-foot .btn-primary').click()
    await page.waitForSelector('.canvas')
    // the requirement asks for rack failures; racks have scenarios of their own
    await page.locator('.right .tab', { hasText: 'Требования' }).click()
    await expect(page.locator('.reqs [data-req="failureTolerance"] select')).toHaveValue('rack')
    await page.locator('.whatif button', { hasText: 'Стойки' }).click()
    await page.locator('.whatif .menu .items button').first().click()
    await expect(page.locator('.whatif .res')).toBeVisible({ timeout: 15_000 })
    await expect(page.locator('.whatif .res')).not.toHaveClass(/crit/)
  })

  test('ten sites wrap into rows and the failure matrix filters thirty-odd scenario families', async ({ page }) => {
    await openWizard(page)
    await sites(page, 10)
    await expect(page.locator('.wiz .site')).toHaveCount(11, { timeout: 20_000 }) // ten sites and the arbiter
    await page.locator('.wiz .cfg input.input').first().fill(uniqueName('ten'))
    await page.locator('.modal-foot .btn-primary').click()
    await page.waitForSelector('.canvas')

    // zones sit in several rows (5 per row), not in one endless line
    // the editor lays the project out when it first opens it; give it a moment, then read the saved draft
    await page.waitForTimeout(2500)
    const rows = await page.evaluate(async () => {
      const id = location.pathname.split('/p/')[1]
      const p = await (await fetch('/api/projects/' + id)).json()
      return new Set(p.architecture.zones.filter((z: { ui?: { y?: number } }) => z.ui?.y !== undefined).map((z: { ui: { y: number } }) => z.ui.y)).size
    })
    expect(rows).toBeGreaterThanOrEqual(3)

    await page.locator('.whatif .w.ghost').last().click()
    const modal = page.locator('.modal')
    await expect(modal.locator('.chips .chip', { hasText: 'Изоляция' })).toBeVisible()
    await modal.locator('.chips .chip', { hasText: 'Изоляция' }).click()
    expect(await modal.locator('tbody tr').count()).toBeGreaterThan(5)
    await modal.getByPlaceholder('Поиск по сценарию').fill('ЦОД-7')
    expect(await modal.locator('tbody tr').count()).toBeGreaterThan(0)
  })
})
