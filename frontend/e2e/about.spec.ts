import { expect } from '@playwright/test'
import { test } from './fixtures'
import { login } from './helpers'

// "About": the version, the license and where the source is come from the server (the crate manifest); nothing of it is written in the page.

test.describe('about', () => {
  test.beforeEach(async ({ page }) => {
    await page.addInitScript(() => { try { localStorage.setItem('ia_tour_off', '1') } catch { /* no storage: the tour may open, the test does not need it closed */ } })
    await login(page)
  })

  test('the Help menu opens a window with the version, the license and the links, from the server', async ({ page }) => {
    const boot = await (await page.request.get('/api/bootstrap')).json()
    expect(boot.about.license).toBe('Apache-2.0')

    await page.locator('.help-menu .trigger').click()
    await page.getByTestId('help-about').click()
    const about = page.getByTestId('about')
    await expect(about).toBeVisible()
    await expect(page.getByTestId('about-version')).toHaveText(`Версия ${boot.version}`)

    // the license is a link to the file in the repository the server names
    const license = page.getByTestId('about-license')
    await expect(license).toContainText(boot.about.license)
    await expect(license).toHaveAttribute('href', `${boot.about.repository}/blob/main/LICENSE`)
    await expect(page.getByTestId('about-third')).toHaveAttribute('href', `${boot.about.repository}/blob/main/THIRD_PARTY_LICENSES.md`)
    await expect(page.getByTestId('about-source')).toHaveAttribute('href', boot.about.repository)
    await expect(page.getByTestId('about-issues')).toHaveAttribute('href', `${boot.about.repository}/issues`)
    // links leave the application without handing it over
    await expect(license).toHaveAttribute('target', '_blank')
    await expect(license).toHaveAttribute('rel', /noopener/)

    // the menu closed itself, the window closes with Esc and gives the focus back
    await expect(page.locator('.help-menu .menu')).toHaveCount(0)
    await page.keyboard.press('Escape')
    await expect(about).toHaveCount(0)
  })

  test('the window is in the language of the interface', async ({ page }) => {
    await page.getByRole('button', { name: 'EN' }).click()
    await page.locator('.help-menu .trigger').click()
    await page.getByTestId('help-about').click()
    await expect(page.getByTestId('about').getByRole('heading', { level: 3 })).toBeVisible()
    await expect(page.getByTestId('about-version')).toHaveText(/^Version \d/)
    await expect(page.getByTestId('about-license')).toContainText('License')
  })
})
