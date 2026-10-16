import { expect } from '@playwright/test'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { test } from './fixtures'
import { createProject, login, projectWith, uniqueName } from './helpers'
import type { Architecture } from '../src/api/types'

// A design with findings (tests/fixtures/plan-spof.json: GSLB and WAF as single instances): the reference architectures that ship have none, and the penalty table of the score dialog needs rows.
const flawed = JSON.parse(readFileSync(join(process.cwd(), 'tests', 'fixtures', 'plan-spof.json'), 'utf8')).architecture as Architecture

test.describe('what makes it feel like a product', () => {
  test('a deletion says what went and offers the way back; the next edit takes the offer away', async ({ page }) => {
    await login(page)
    await createProject(page, uniqueName('undo'), '2 ЦОД + witness')
    const cards = page.locator('.arch-node')
    await expect(cards.first()).toBeVisible()
    const before = await cards.count()

    await cards.filter({ hasText: 'Kafka' }).first().click()
    await page.keyboard.press('Delete')
    await expect(cards).toHaveCount(before - 1)
    const toast = page.locator('.toast', { hasText: 'удалён' })
    await expect(toast).toBeVisible()
    await toast.getByTestId('toast-action').click()
    await expect(cards).toHaveCount(before)
    await expect(toast).toHaveCount(0)

    // another edit makes the offer stale: only the last action can be undone
    await cards.filter({ hasText: 'Kafka' }).first().click()
    await page.keyboard.press('Delete')
    await expect(page.locator('.toast', { hasText: 'удалён' })).toBeVisible()
    await cards.first().click()
    await page.keyboard.press('Delete')
    await expect(page.locator('.toast .act')).toHaveCount(1) // the new deletion's own, not both
  })

  test('an empty diagram offers three ways to begin, and each leads somewhere', async ({ page }) => {
    await login(page)
    await createProject(page, uniqueName('blank'))
    const empty = page.getByTestId('canvas-empty')
    await expect(empty).toBeVisible()
    await expect(empty.locator('.start')).toHaveCount(2) // no key for the AI on this stack: the wizard and the templates
    await empty.locator('.start', { hasText: 'Мастер размещения' }).click()
    await expect(page.locator('.wiz')).toBeVisible({ timeout: 15_000 })
    await page.keyboard.press('Escape')
    await empty.locator('.start', { hasText: 'Эталонная архитектура' }).click()
    await expect(page.locator('.library')).toBeVisible()
  })

  test('the project list shows the shape of what is coming while it loads', async ({ page }) => {
    await login(page)
    // a slow answer, from the real server: only held back
    await page.route('**/api/projects', async (route) => {
      if (route.request().method() === 'GET') await new Promise((r) => setTimeout(r, 1200))
      await route.continue()
    })
    await page.goto('/')
    await expect(page.getByTestId('projects-skeleton')).toBeVisible()
    await expect(page.getByTestId('projects-skeleton')).toHaveCount(0, { timeout: 10_000 })
    await expect(page.locator('.project').first()).toBeVisible()
  })

  test('the editor shows its shape while the project loads', async ({ page }) => {
    await login(page)
    await createProject(page, uniqueName('slowload'), '2 ЦОД + witness')
    const url = page.url()
    await page.route(/\/api\/projects\/[0-9a-f-]+$/, async (route) => {
      if (route.request().method() === 'GET') await new Promise((r) => setTimeout(r, 1200))
      await route.continue()
    })
    await page.goto(url)
    await expect(page.getByTestId('editor-skeleton')).toBeVisible()
    await expect(page.locator('.arch-node').first()).toBeVisible({ timeout: 15_000 })
    await expect(page.getByTestId('editor-skeleton')).toHaveCount(0)
  })

  test('the score dialog keeps each label on one line, the rule ids and the status pills included', async ({ page }) => {
    await login(page)
    const id = await projectWith(page, uniqueName('score'), flawed)
    await page.goto(`/p/${id}`)
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await page.locator('.eh .score').click()
    await expect(page.locator('.modal table tr').first()).toBeVisible()
    // how many lines the text of each label really takes (a pill has a fixed height, a cell takes the height of its row: neither says it)
    const cramped = await page.evaluate(() => {
      const lines = (el: Element) => { const r = document.createRange(); r.selectNodeContents(el); return new Set(Array.from(r.getClientRects()).map((x) => Math.round(x.top))).size }
      return Array.from(document.querySelectorAll('.modal .tag, .modal td.mono')).filter((e) => lines(e) > 1).map((e) => e.textContent?.trim())
    })
    expect(cramped).toEqual([])
  })
})
