import { expect, type Page } from '@playwright/test'
import { test } from './fixtures'
import { createProject, login, uniqueName } from './helpers'

// The panel of a failure says what it costs (four tiles), who it hits (a list that takes to the card), what the service does at each step (columns
// over the track), and can be handed on: a link that opens the diagram with that failure, and the account of it as Markdown. Several failures at
// once are a scenario of their own.

test.describe('the failure panel', () => {
  test.beforeEach(async ({ page }) => {
    await login(page)
    // a failure that has just been run plays itself; these tests look at the panel, not at the story
    expect((await page.request.put('/api/admin/settings', { data: { changes: { 'editor.failureLeadMs': 0 } } })).ok()).toBeTruthy()
    await createProject(page, uniqueName('simpanel'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
  })
  test.afterEach(async ({ page }) => {
    await page.request.put('/api/admin/settings', { data: { changes: { 'editor.failureLeadMs': 2400 } } })
  })

  /** Runs the failure of a site and waits until its story has been told (the page was loaded before the administrator's lead was set to 0, so it still plays itself). */
  const run = async (page: Page, zone: string) => {
    await page.locator('.whatif button', { hasText: zone }).first().click()
    await expect(page.locator('[data-testid="simcard"]')).toBeVisible({ timeout: 15_000 })
    const count = await page.locator('.steps > li').count()
    await expect(page.locator('.steps > li.cur')).toHaveAttribute('data-step', String(count - 1), { timeout: 25_000 })
  }

  test('four tiles say what the failure costs, and follow the picture on screen', async ({ page }) => {
    await run(page, 'ЦОД-1')
    const tiles = page.locator('[data-testid="sim-tiles"] .tile')
    await expect(tiles).toHaveCount(4)
    await expect(page.locator('[data-tile="rto"]')).toContainText('35 с')
    await expect(page.locator('[data-tile="rto"]')).toContainText('автоматически')
    await expect(page.locator('[data-tile="rpo"]')).toContainText('5 с')
    await expect(page.locator('[data-tile="capacity"]')).toContainText('%')
    const settled = await page.locator('[data-tile="affected"] .v').innerText()
    expect(settled).toMatch(/^\d+ из \d+$/)
    expect(Number(settled.split(' ')[0])).toBeGreaterThan(0)

    // before the failure nothing is hit: the tile says so, the capacity has nothing to say yet
    await page.locator('.steps > li').first().click()
    await expect(page.locator('[data-tile="affected"] .v')).toHaveText(/^0 из \d+$/)
    await expect(page.locator('[data-tile="affected"]')).toHaveClass(/ok/)
    await expect(page.locator('[data-tile="capacity"] .v')).toHaveText('—')
    await page.locator('.steps > li').last().click()
    await expect(page.locator('[data-tile="affected"] .v')).toHaveText(settled)
  })

  test('the list of who is hit takes to the card, and opens it', async ({ page }) => {
    await run(page, 'ЦОД-1')
    const impact = page.locator('[data-testid="sim-impact"]')
    await expect(impact.locator('[data-tier="out"]')).toBeVisible()
    await expect(impact.locator('[data-tier="took"]')).toContainText('PostgreSQL replica')
    // the list keeps to the setting (editor.impactShown): the rest is behind "ещё N"
    const more = impact.locator('[data-tier="out"] .more')
    await expect(more).toBeVisible()
    const before = await impact.locator('[data-tier="out"] [data-testid="sim-hit"]').count()
    await more.click()
    expect(await impact.locator('[data-tier="out"] [data-testid="sim-hit"]').count()).toBeGreaterThan(before)

    await impact.locator('[data-tier="took"] [data-testid="sim-hit"]', { hasText: 'PostgreSQL replica' }).click()
    await expect(page.locator('.inspector')).toBeVisible()
    await expect(page.locator('.inspector')).toContainText('PostgreSQL replica')
    await expect(page.locator('.arch-node.selected, .vue-flow__node.selected', { hasText: 'PostgreSQL replica' }).first()).toBeVisible()
  })

  test('a column for every step shows the share of the requests served; a click on it is a step on the track', async ({ page }) => {
    await run(page, 'ЦОД-1')
    const cols = page.locator('[data-testid="sim-chart"] .col')
    const steps = page.locator('.steps > li')
    expect(await cols.count()).toBe(await steps.count())
    expect(await cols.count()).toBeGreaterThanOrEqual(4)
    // normal operation is whole, the moment of the failure is not served at all, the last step is the settled outcome
    await expect(cols.first()).toHaveClass(/\bok\b/)
    await expect(cols.nth(1)).toHaveClass(/\bcrit\b/)
    const last = await cols.last().evaluate((e) => e.className)
    expect(last).toMatch(/risk|ok/)
    await expect(cols.last()).toHaveClass(/cur/)
    // the height is the share served
    const h = async (i: number) => (await cols.nth(i).boundingBox())!.height
    expect(await h(0)).toBeGreaterThan(await h(1))
    // a click on the chart seeks, the same as on the track
    const box = (await cols.nth(1).boundingBox())!
    await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2 - 6)
    await expect(page.locator('.steps > li.cur')).toHaveAttribute('data-step', '1')
    await expect(cols.nth(1)).toHaveClass(/cur/)
    await expect(page.locator('[data-testid="sim-state"]')).toHaveClass(/down/)
  })

  test('a link opens the diagram with the failure on screen, once; the account is Markdown', async ({ page, context, baseURL }) => {
    await context.grantPermissions(['clipboard-read', 'clipboard-write'], { origin: baseURL })
    await run(page, 'ЦОД-1')

    await page.locator('[data-testid="sim-copy-md"]').click()
    await expect(page.locator('.toast', { hasText: 'Markdown' })).toBeVisible()
    const md = await page.evaluate(() => navigator.clipboard.readText())
    expect(md).toMatch(/^# Симуляция: ЦОД-1/)
    expect(md).toContain('**Итог:**')
    expect(md).toContain('### до отказа · 100%')
    expect(md).toContain('## Кого затронуло')
    expect(md.split('\n').filter((l) => l.startsWith('### ')).length).toBe(await page.locator('.steps > li').count())

    await page.locator('[data-testid="sim-copy-link"]').click()
    await expect(page.locator('.toast', { hasText: 'Ссылка скопирована' })).toBeVisible()
    const link = await page.evaluate(() => navigator.clipboard.readText())
    const url = new URL(link)
    expect(url.pathname).toBe(new URL(page.url()).pathname)
    expect([...url.searchParams.keys()]).toEqual(['sim'])

    // someone opens it: the failure is on screen, and the parameter is spent
    await page.goto(link)
    await expect(page.locator('[data-testid="simcard"]')).toBeVisible({ timeout: 20_000 })
    await expect(page.locator('.right .tab.active')).toHaveText(/Отказ/)
    await expect(page.locator('[data-testid="simcard"] h2')).toContainText('ЦОД-1')
    await expect.poll(() => new URL(page.url()).search).toBe('')
    // … and a reload does not bring it back
    await page.reload()
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await expect(page.locator('[data-testid="simcard"]')).toHaveCount(0)

    // a link that names what this diagram does not have says so instead of showing nothing
    const stale = Buffer.from(JSON.stringify({ type: 'zone', zoneId: 'no-such-zone' })).toString('base64url')
    await page.goto(`${new URL(page.url()).pathname}?sim=${stale}`)
    await expect(page.locator('.toast', { hasText: 'не подходит к этой схеме' })).toBeVisible({ timeout: 20_000 })
    await expect(page.locator('[data-testid="simcard"]')).toHaveCount(0)
    // and a parameter that is not a scenario at all is the same
    await page.goto(`${new URL(page.url()).pathname}?sim=%25%25garbage`)
    await expect(page.locator('.toast', { hasText: 'не подходит к этой схеме' })).toBeVisible({ timeout: 20_000 })
  })

  test('copying works where the page has no clipboard API: an intranet tool served over plain http', async ({ page }) => {
    // a page that is not a secure context has no `navigator.clipboard`: the text goes through a field that is selected and copied
    await page.addInitScript(() => {
      Object.defineProperty(navigator, 'clipboard', { value: undefined, configurable: true })
      ;(window as unknown as { __copied: string[] }).__copied = []
      document.addEventListener('copy', () => (window as unknown as { __copied: string[] }).__copied.push(document.getSelection()?.toString() ?? ''))
    })
    await page.reload()
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await run(page, 'ЦОД-1')
    await page.locator('[data-testid="sim-copy-md"]').click()
    await expect(page.locator('.toast', { hasText: 'Markdown' })).toBeVisible()
    const copied = await page.evaluate(() => (window as unknown as { __copied: string[] }).__copied)
    expect(copied).toHaveLength(1)
    expect(copied[0]).toMatch(/^# Симуляция: ЦОД-1/)
    // and the focus is where it was: the button that was pressed
    await expect(page.locator('[data-testid="sim-copy-md"]')).toBeFocused()
  })

  test('several failures at once: a site and a node together are a scenario of their own', async ({ page }) => {
    const bar = page.locator('[data-testid="whatif"]')
    // each of them alone is survived
    await run(page, 'ЦОД-1')
    await expect(page.locator('[data-testid="sim-state"]')).not.toHaveClass(/down/)

    // the picker starts from the failure on screen: "and if this fails too?"
    await bar.locator('[data-testid="whatif-compose"]').click()
    const menu = bar.locator('.menu.compose')
    await expect(menu.locator('.chk', { hasText: 'ЦОД-1 · Москва' }).locator('input')).toBeChecked()
    await expect(menu.locator('.chk', { hasText: 'ЦОД-2 · Москва' }).locator('input')).not.toBeChecked()
    await menu.locator('.chk', { hasText: 'PostgreSQL replica' }).locator('input').check()
    await expect(menu.locator('[data-testid="compose-run"]')).toContainText('2')
    await menu.locator('[data-testid="compose-run"]').click()

    // together they leave no database: the service is down, and the failure is named by both
    await expect(page.locator('[data-testid="sim-state"]')).toHaveClass(/down/, { timeout: 15_000 })
    await expect(page.locator('[data-testid="simcard"] h2')).toContainText('PostgreSQL replica')
    await expect(page.locator('[data-testid="simcard"] h2')).toContainText('ЦОД-1')
    await expect(bar.locator('.res')).toContainText('сервис недоступен')
    await expect(page.locator('[data-tile="rto"]')).toContainText('вручную')
    // the picker remembers what is on screen, and nothing is ticked after the failure is closed
    await bar.locator('[data-testid="whatif-compose"]').click()
    await expect(menu.locator('.chk', { hasText: 'PostgreSQL replica' }).locator('input')).toBeChecked()
    await menu.locator('button', { hasText: 'Снять всё' }).click()
    await expect(menu.locator('[data-testid="compose-run"]')).toBeDisabled()

    // what is selected on the diagram is added in one click (the click on the diagram closes the picker, so the selection comes first)
    await page.locator('.arch-node', { hasText: 'Vault' }).first().click()
    await expect(menu).toHaveCount(0)
    await bar.locator('[data-testid="whatif-compose"]').click()
    await expect(menu.locator('.chk', { hasText: 'Vault' }).first().locator('input')).not.toBeChecked()
    await menu.locator('[data-testid="compose-selected"]').click()
    await expect(menu.locator('.chk', { hasText: 'Vault' }).first().locator('input')).toBeChecked()
    await menu.locator('.chk', { hasText: 'ЦОД-2 · Москва' }).locator('input').check()
    await menu.locator('[data-testid="compose-run"]').click()
    await expect(page.locator('[data-testid="simcard"] h2')).toContainText('Vault')
  })
})
