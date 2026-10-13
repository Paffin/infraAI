import { readFileSync } from 'node:fs'
import { expect, test } from '@playwright/test'
import { createProject, login, uniqueName } from './helpers'

test.describe('export', () => {
  test.beforeEach(async ({ page }) => {
    await login(page)
  })

  test('infrastructure as code reports what it generated and what it could not, and filling the gap closes it', async ({ page }) => {
    await createProject(page, uniqueName('export'), '3 AZ в одном регионе')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await page.getByRole('button', { name: 'Экспорт' }).first().click()
    await page.locator('.formats button', { hasText: 'Манифесты Kubernetes' }).click()

    // honest report: generated, not generated (with the reason), prerequisites
    await expect(page.locator('[data-test="export-summary"]')).toBeVisible({ timeout: 20_000 })
    const generatedBefore = await page.locator('.row-g').count()
    expect(generatedBefore).toBeGreaterThan(2)
    const gap = page.locator('[data-test="export-skipped-needs_input"]', { hasText: 'Kubernetes · app' })
    await expect(gap).toBeVisible()
    await page.locator('[data-test="export-tab-setup"]').click()
    await expect(page.locator('.pane table').first()).toContainText('strimzi')
    await page.locator('[data-test="export-tab-coverage"]').click()

    // "show" leads to the node whose property is missing
    await gap.getByRole('button', { name: 'Заполнить' }).click()
    await expect(page.locator('.modal')).toHaveCount(0)
    await expect(page.locator('.inspector header b')).toContainText('Kubernetes · app')

    // the inspector opened its group and focused the very field: the container image is an ordinary property
    const image = page.locator('.inspector [data-prop="image"] input')
    await expect(image).toBeFocused()
    await image.fill('registry.example.com/shop/app:1.4.2')
    await expect(page.locator('.crumbs .meta')).toContainText('сохранено', { timeout: 10_000 })

    await page.getByRole('button', { name: 'Экспорт' }).first().click()
    await page.locator('.formats button', { hasText: 'Манифесты Kubernetes' }).click()
    await expect(page.locator('[data-test="export-summary"]')).toBeVisible({ timeout: 20_000 })
    await expect(page.locator('[data-test="export-skipped-needs_input"]', { hasText: 'Kubernetes · app' })).toHaveCount(0)
    expect(await page.locator('.row-g').count()).toBeGreaterThan(generatedBefore)
    await page.locator('[data-test="export-tab-files"]').click()
    await page.locator('.tabs2 button', { hasText: 'app' }).first().click()
    await expect(page.locator('.code')).toContainText('registry.example.com/shop/app:1.4.2')
  })

  test('the diagram as an SVG file', async ({ page }) => {
    await createProject(page, uniqueName('svg'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await page.getByRole('button', { name: 'Экспорт' }).first().click()
    await page.locator('.formats button', { hasText: 'Схема (SVG)' }).click()
    await expect(page.locator('.svgprev img')).toBeVisible()
    const link = page.locator('a.btn-primary', { hasText: 'Скачать' }).first()
    const [download] = await Promise.all([page.waitForEvent('download'), link.click()])
    expect(download.suggestedFilename()).toMatch(/\.svg$/)
    const svg = readFileSync((await download.path())!, 'utf8')
    expect(svg.startsWith('<?xml')).toBe(true)
    expect(svg).toContain('<svg')
    expect(svg).toContain('PostgreSQL')
    // switching quickly between formats shows the format that was chosen last
    await page.locator('.formats button', { hasText: 'Манифесты Kubernetes' }).click()
    await page.locator('.formats button', { hasText: 'Схема (SVG)' }).click()
    await expect(page.locator('.svgprev img')).toBeVisible()
    await page.waitForTimeout(1500)
    await expect(page.locator('.svgprev img')).toBeVisible()
    await expect(page.locator('[data-test="export-summary"]')).toHaveCount(0)
  })
})
