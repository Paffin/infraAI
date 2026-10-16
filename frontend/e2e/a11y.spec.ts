import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { expect, type Page } from '@playwright/test'
import { test } from './fixtures'
import { createProject, login, openAnalysis, projectWith, settled, uniqueName } from './helpers'
import type { Architecture } from '../src/api/types'

/**
 * Accessibility, checked by axe-core on the screens people spend their time on: every field has a name, headings and
 * landmarks give the page a shape a screen reader can walk, text keeps its contrast (WCAG 2.1 A and AA).
 *
 * The page's Content-Security-Policy forbids inline scripts, and it stays that way: only this spec lifts it for its
 * own test browser, to hand axe to the page.
 */
test.use({ bypassCSP: true })

// axe judges what is on the screen at that moment: a card halfway through its entrance would be judged at half its colour
test.beforeEach(async ({ page }) => { await page.addInitScript(() => { try { localStorage.setItem('ia_motion', 'off') } catch { /* no storage: the default motion applies */ } }) })

const axe = readFileSync(createRequire(import.meta.url).resolve('axe-core/axe.min.js'), 'utf8')

interface Violation { id: string; impact: string | null; help: string; nodes: { target: string[]; failureSummary?: string }[] }

async function violations(page: Page): Promise<string[]> {
  await settled(page) // a colour that is halfway through a transition is judged at the colour it has at that moment
  await page.evaluate(axe)
  const found: Violation[] = await page.evaluate(async () => {
    // @ts-expect-error axe is injected above
    // what a simulation fades out on purpose (`.dim`), or what the selection steps back from (`.faded`; the "Focus" button turns that off), is not
    // for reading at that moment: it is back at full strength when the simulation closes or the selection goes
    const res = await window.axe.run({ include: [['body']], exclude: [['.dim'], ['.faded']] }, { resultTypes: ['violations'], runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'best-practice'] } })
    return res.violations
  })
  return found.map((v) => `${v.id} [${v.impact}] ×${v.nodes.length}: ${v.help} — e.g. ${v.nodes[0]?.target.join(' ')} ${(v.nodes[0]?.failureSummary ?? '').split('\n')[1] ?? ''}`.trim())
}

const clean = async (page: Page, screen: string) => expect(await violations(page), screen).toEqual([])

test.describe('accessibility (axe-core)', () => {
  test('the login page', async ({ page }) => {
    await page.goto('/login')
    await expect(page.locator('#email')).toBeVisible()
    await clean(page, 'login')
  })

  test('the project list and the new-project wizard', async ({ page }) => {
    await login(page)
    await expect(page.locator('.project').first()).toBeVisible()
    await clean(page, 'projects')
    await page.getByRole('button', { name: 'Новый проект' }).first().click()
    await page.locator('.wizard-card').click()
    await expect(page.locator('.wiz .site').first()).toBeVisible({ timeout: 20_000 })
    await clean(page, 'placement wizard')
  })

  test('the editor: every panel, the inspector, a simulation and the export dialog', async ({ page }) => {
    await login(page)
    await createProject(page, uniqueName('a11y'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await clean(page, 'editor')

    for (const tab of ['Требования', 'Проверки']) {
      await page.locator('.right .tab', { hasText: tab }).click()
      await clean(page, `tab ${tab}`)
    }
    for (const view of ['Ёмкость', 'Доступность', 'Стоимость', 'Площадки'] as const) {
      await openAnalysis(page, view)
      await clean(page, `analysis ${view}`)
    }
    await page.locator('.right .tab', { hasText: 'Проверки' }).click()

    await page.locator('.arch-node').first().click()
    await expect(page.locator('.inspector')).toBeVisible()
    await clean(page, 'inspector')
    await page.keyboard.press('Escape')

    await page.locator('.whatif .w', { hasText: 'ЦОД-1' }).first().click()
    await expect(page.getByTestId('simcard')).toBeVisible()
    await clean(page, 'simulation')
    await page.locator('.whatif .x').click()

    await page.getByRole('button', { name: /Экспорт/ }).first().click()
    await expect(page.locator('.modal')).toBeVisible()
    await clean(page, 'export')
  })

  test('the arranged editor: the rail, the failure window, the several-at-once picker and the library with its folded categories', async ({ page }) => {
    await login(page)
    expect((await page.request.put('/api/admin/settings', { data: { changes: { 'editor.failureLeadMs': 0 } } })).ok()).toBeTruthy()
    try {
      await createProject(page, uniqueName('a11y-panels'), '2 ЦОД + witness')
      await expect(page.locator('.arch-node').first()).toBeVisible()
      // the library: a category folded, a search with a limit
      await page.getByTestId('lib-cat-edge').click()
      await clean(page, 'library with a folded category')
      await page.locator('.library .search input').fill('a')
      await clean(page, 'library search')
      await page.locator('.library .search input').fill('')
      // a failure: the tiles, the list of who is hit, the window
      await page.locator('.whatif .w', { hasText: 'ЦОД-1' }).first().click()
      await expect(page.getByTestId('simcard')).toBeVisible()
      await expect(page.getByTestId('sim-impact')).toBeVisible()
      await clean(page, 'failure panel with tiles and the list of who is hit')
      await page.getByTestId('sim-detach').click()
      await expect(page.getByTestId('float-panel')).toBeVisible()
      await clean(page, 'failure window')
      await page.getByTestId('float-fold').click()
      await clean(page, 'folded failure window')
      // the picker of several failures at once, and the rail
      await page.getByTestId('whatif-compose').click()
      await expect(page.locator('.whatif .menu.compose')).toBeVisible()
      await clean(page, 'several at once')
      await page.keyboard.press('Escape')
      await page.getByTestId('right-fold').click()
      await clean(page, 'right panel folded to the rail')
    } finally {
      await page.request.put('/api/admin/settings', { data: { changes: { 'editor.failureLeadMs': 2400 } } })
    }
  })

  test('the checks with "what to improve first": the figures keep their contrast and every button says what it applies', async ({ page }) => {
    await login(page)
    // the design the plan was recorded for: two data centers whose GSLB and WAF run as a single instance
    const { architecture } = JSON.parse(readFileSync('tests/fixtures/plan-spof.json', 'utf8')) as { architecture: Architecture }
    const id = await projectWith(page, uniqueName('a11y-rec'), architecture)
    await page.goto(`/p/${id}`)
    await expect(page.getByTestId('recommendations').locator('[data-rec]').first()).toBeVisible({ timeout: 20_000 })
    await clean(page, 'recommendations')
  })

  test('the admin panel', async ({ page }) => {
    await login(page)
    for (const path of ['settings', 'catalog/kind', 'catalog/versions', 'placement/pattern']) {
      await page.goto('/admin/' + path)
      await expect(page.locator('#adm-main')).toBeVisible()
      await page.waitForTimeout(600)
      await clean(page, 'admin ' + path)
    }
  })
})
