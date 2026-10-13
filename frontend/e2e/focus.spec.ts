import { expect, type Page } from '@playwright/test'
import { test } from './fixtures'
import { createProject, login, uniqueName } from './helpers'

const opacity = (page: Page, selector: string) => page.locator(selector).first().evaluate((el) => parseFloat(getComputedStyle(el).opacity))

test.describe('the selection puts what it touches in focus, and many sites do not make a hairball', () => {
  test.beforeEach(async ({ page }) => {
    await login(page)
  })

  test('a selected card lights its links and neighbours and the rest steps back; the toggle turns it off and is remembered', async ({ page }) => {
    await createProject(page, uniqueName('focus'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await expect(page.locator('g.faded')).toHaveCount(0)

    const card = page.locator('.arch-node', { hasText: 'Kafka' }).first()
    await card.click()
    await expect(card).toHaveClass(/selected/)
    // its links stand out, the other links and cards are quiet, and the card itself is not
    await expect.poll(() => page.locator('g.emph').count()).toBeGreaterThan(0)
    expect(await page.locator('g.faded').count()).toBeGreaterThan(5)
    expect(await page.locator('.arch-node.faded').count()).toBeGreaterThan(5)
    await expect(card).not.toHaveClass(/faded/)
    expect(await opacity(page, 'g.faded')).toBeLessThan(0.3)
    expect(await opacity(page, 'g.emph')).toBeGreaterThan(0.9)
    // the cards at the other end of its links stay as they were
    const ends = await page.evaluate(() => {
      const emphEdges = document.querySelectorAll('g.emph').length
      return { emphEdges, notFaded: document.querySelectorAll('.arch-node:not(.faded)').length }
    })
    expect(ends.notFaded).toBeGreaterThanOrEqual(ends.emphEdges > 0 ? 2 : 1)

    // nothing selected: nothing is quiet
    await page.keyboard.press('Escape')
    await page.locator('.vue-flow__pane').click({ position: { x: 30, y: 400 } })
    await expect(page.locator('g.faded')).toHaveCount(0)

    // switched off, a selection changes nothing around it; the choice survives a reload
    const toggle = page.getByTestId('focus-toggle')
    await expect(toggle).toHaveAttribute('aria-pressed', 'true')
    await toggle.click()
    await expect(toggle).toHaveAttribute('aria-pressed', 'false')
    await card.click()
    await expect(card).toHaveClass(/selected/)
    await expect(page.locator('g.faded')).toHaveCount(0)
    await expect(page.locator('.arch-node.faded')).toHaveCount(0)
    await page.reload()
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await expect(page.getByTestId('focus-toggle')).toHaveAttribute('aria-pressed', 'false')
    await page.getByTestId('focus-toggle').click()
  })

  test('what is selected stays visible however far the diagram is zoomed out', async ({ page }) => {
    await createProject(page, uniqueName('focus-ten'), '10 площадок')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await expect.poll(async () => page.locator('.canvas').getAttribute('data-lod')).toMatch(/mini|compact/)
    const card = page.locator('.arch-node', { hasText: 'etcd' }).nth(3)
    await card.click({ force: true })
    await expect(card).toHaveClass(/selected/)
    // an outline that does not shrink with the zoom: at a quarter of the size it is still a few pixels
    const outline = await card.evaluate((el) => parseFloat(getComputedStyle(el).outlineWidth))
    const zoom = await page.locator('.canvas').evaluate((el) => parseFloat(getComputedStyle(el).getPropertyValue('--zoom')))
    expect(zoom).toBeLessThan(0.62)
    expect(outline * zoom).toBeGreaterThan(1.5)
    await expect.poll(() => page.locator('g.emph').count()).toBeGreaterThan(0)
  })

  test('with many sites the links between them step back when zoomed out, and the number of sites is a setting', async ({ page }) => {
    await createProject(page, uniqueName('dense'), '5 площадок')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    const canvas = page.locator('.canvas')
    await expect(canvas).toHaveAttribute('data-dense', '1')
    await expect.poll(async () => canvas.getAttribute('data-lod')).toMatch(/mini|compact/)
    // the links between sites are pale, the links inside a site are not
    await expect.poll(async () => page.locator('g.cross').count()).toBeGreaterThan(3)
    expect(await opacity(page, 'g.cross')).toBeLessThan(0.45)
    const inside = await page.evaluate(() => {
      const g = Array.from(document.querySelectorAll('.vue-flow__edge g')).find((e) => !e.classList.contains('cross') && !e.classList.contains('quiet') && !e.classList.contains('dim'))
      return g ? parseFloat(getComputedStyle(g).opacity) : -1
    })
    expect(inside).toBeGreaterThan(0.9)

    // the threshold is the administrator's: from more sites than this design has, nothing is quiet
    const put = (n: number) => page.request.put('/api/admin/settings', { data: { changes: { 'editor.quietEdgesSites': n } } })
    try {
      expect((await put(20)).ok()).toBeTruthy()
      await page.reload()
      await expect(page.locator('.arch-node').first()).toBeVisible()
      await expect(page.locator('.canvas')).toHaveAttribute('data-dense', '0')
      expect(await opacity(page, 'g.cross')).toBeGreaterThan(0.9)
    } finally {
      await put(4)
    }
  })
})
