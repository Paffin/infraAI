import { expect } from '@playwright/test'
import { test } from './fixtures'
import { createProject, login, openAnalysis, projectWith, uniqueName } from './helpers'
import type { Architecture } from '../src/api/types'

test.describe('the sites of a design: where things run and what the loss of each does', () => {
  test.beforeEach(async ({ page }) => {
    await login(page)
  })

  test('every site is a card with its services, its racks and the verdict of the simulator on losing it', async ({ page }) => {
    await createProject(page, uniqueName('sites'), '5 площадок')
    await openAnalysis(page, 'Площадки')
    const sites = page.locator('.sites .site')
    await expect(sites.first()).toBeVisible({ timeout: 15_000 })
    expect(await sites.count()).toBeGreaterThanOrEqual(5)
    // the verdict of each site is what the engine's scenario for that zone says: a state and the facts behind it
    const verdicts = page.locator('.sites .site .verdict')
    await expect(verdicts.first()).toBeVisible()
    expect(await verdicts.count()).toBeGreaterThanOrEqual(5)
    await expect(verdicts.first()).toContainText('Если откажет:')
    await expect(verdicts.first().locator('.state')).toHaveText(/сервис жив|деградация|сервис недоступен/)
    // services are chips of their products, and there are some for every site
    const chips = sites.first().locator('.chip-s')
    expect(await chips.count()).toBeGreaterThan(2)
    // the sites are grouped by the disaster domain they declare
    expect(await page.locator('.sites .domain h3').count()).toBeGreaterThanOrEqual(1)
    // the four views of the analysis fit their strip: nothing is cut
    const cut = await page.evaluate(() => Array.from(document.querySelectorAll<HTMLElement>('.seg .segbtn')).filter((b) => b.scrollWidth > b.clientWidth + 1).map((b) => b.textContent?.trim()))
    expect(cut).toEqual([])
  })

  test('"Show" brings the canvas to the site and "Fail it" replays its loss', async ({ page }) => {
    await createProject(page, uniqueName('sites-act'), '2 ЦОД + witness')
    await openAnalysis(page, 'Площадки')
    const site = page.locator('.sites .site').first()
    await expect(site).toBeVisible({ timeout: 15_000 })
    await expect(page.locator('.whatif .res')).toHaveCount(0)
    await site.getByRole('button', { name: 'Отказ' }).click()
    await expect(page.locator('.whatif .res')).toBeVisible({ timeout: 15_000 })
    await expect(site).toHaveClass(/\bon\b/)
    await page.locator('.whatif .x').click()
    await expect(site).not.toHaveClass(/\bon\b/)

    await site.getByRole('button', { name: 'Показать' }).click()
    await expect(page.locator('.inspector')).toBeVisible()
  })

  test('a design with a single site says there is nothing to compare', async ({ page }) => {
    // the single-data-center template also has a site for the backups: leave only the data center
    const tpl = await (await page.request.get('/api/templates/single-dc-racks')).json()
    const arch = tpl.architecture as Architecture
    const dropped = new Set(arch.zones.filter((z) => z.role === 'backup' || /backup|резерв/i.test(z.name)).map((z) => z.id))
    expect(dropped.size).toBeGreaterThan(0)
    const nodes = new Set(arch.nodes.filter((n) => n.zoneId && dropped.has(n.zoneId)).map((n) => n.id))
    arch.zones = arch.zones.filter((z) => !dropped.has(z.id))
    arch.nodes = arch.nodes.filter((n) => !nodes.has(n.id))
    arch.edges = arch.edges.filter((e) => !nodes.has(e.source) && !nodes.has(e.target))
    const id = await projectWith(page, uniqueName('sites-one'), arch)
    await page.goto(`/p/${id}`)
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await openAnalysis(page, 'Площадки')
    await expect(page.getByTestId('sites-few')).toBeVisible({ timeout: 15_000 })
    await expect(page.locator('.sites .site')).toHaveCount(0)
  })
})
