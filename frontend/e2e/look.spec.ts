import { expect } from '@playwright/test'
import { test } from './fixtures'
import { createProject, login, uniqueName } from './helpers'

test.describe('the look of the diagram: cards, zones, labels and the legend', () => {
  test.beforeEach(async ({ page }) => {
    await login(page)
  })

  test('every card wears the icon of its kind, the library does too, and a component with several instances is drawn as a stack', async ({ page }) => {
    await createProject(page, uniqueName('look'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    const cards = await page.locator('.arch-node').count()
    expect(cards).toBeGreaterThan(15)
    expect(await page.locator('.arch-node .kchip svg.ic').count()).toBe(cards)
    // not one placeholder: the catalog names an icon for each of the kinds in this design, and they are not all the same
    const icons = await page.locator('.arch-node .kchip svg.ic').evaluateAll((els) => new Set(els.map((e) => e.getAttribute('class')?.split(' ').find((c) => c.startsWith('lucide-') && !c.endsWith('-icon')))).size)
    expect(icons).toBeGreaterThan(8)
    // the library offers the same faces, and a zone type its own
    expect(await page.locator('.library .kind .kchip svg.ic').count()).toBeGreaterThan(20)
    expect(await page.locator('.library .zchip svg').count()).toBeGreaterThan(2)
    // the card with several instances is a stack; a card of one instance is not
    await expect(page.locator('.arch-node.stack-3').first()).toBeVisible()
    const stack = await page.locator('.arch-node.stack-3').first().evaluate((el) => getComputedStyle(el).boxShadow)
    const single = await page.locator('.arch-node:not(.stack-2):not(.stack-3)').first().evaluate((el) => getComputedStyle(el).boxShadow)
    expect(stack.split('rgb').length).toBeGreaterThan(single.split('rgb').length)
  })

  test('a zone has its type\'s icon, its role, its tier and how many cards stand in it', async ({ page }) => {
    await createProject(page, uniqueName('look-zone'), '2 ЦОД + witness')
    await expect(page.locator('.arch-zone').first()).toBeVisible()
    const dc = page.locator('.arch-zone', { hasText: 'ЦОД-1' }).first()
    await expect(dc.locator('.zico svg')).toBeVisible()
    await expect(dc.locator('.ztag.role-active')).toHaveText('active')
    await expect(dc.locator('.ztag.tier')).toContainText('Tier III')
    await expect(dc.locator('.zmeta')).toContainText(/\d+ узл/)
    const witness = page.locator('.arch-zone', { hasText: 'Площадка 3' }).first()
    await expect(witness.locator('.ztag.role-witness')).toBeVisible()
  })

  test('the legend says what the lines, the cards, the zones and the failure particles mean, and closes with Esc or a click elsewhere', async ({ page }) => {
    await createProject(page, uniqueName('look-legend'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    const toggle = page.getByTestId('legend-toggle')
    await expect(page.getByTestId('legend')).toHaveCount(0)
    await toggle.click()
    const legend = page.getByTestId('legend')
    await expect(legend).toBeVisible()
    await expect(toggle).toHaveAttribute('aria-expanded', 'true')
    // every link type of the catalog, drawn the way it is, with what it means
    const types = await page.evaluate(async () => (await (await fetch('/api/bootstrap')).json()).catalog.edgeTypes.length)
    expect(types).toBeGreaterThanOrEqual(6)
    await expect(legend.locator('section').first().locator('li')).toHaveCount(types)
    // every link type explains itself in its own words (the two kinds of replication are not the same thing)
    const says = await legend.locator('section').first().locator('li .tx').allInnerTexts()
    expect(new Set(says).size).toBe(types)
    await expect(legend).toContainText('Синхронная репликация')
    await expect(legend).toContainText('Запросы идут от источника к цели')
    await expect(legend).toContainText('Карточки и зоны')
    await expect(legend).toContainText('При отказе')
    await expect(legend).toContainText('обход')
    // no key of a translation shows through
    await expect(legend).not.toContainText('legend.')
    await page.keyboard.press('Escape')
    await expect(legend).toHaveCount(0)
    await toggle.click()
    await expect(legend).toBeVisible()
    await page.locator('.vue-flow__pane').click({ position: { x: 40, y: 500 } })
    await expect(legend).toHaveCount(0)
  })

  test('zoomed out, a card name is drawn as large as its longest word allows: no word is cut in the middle', async ({ page }) => {
    for (const template of ['1 ЦОД · 3 стойки', '2 ЦОД + witness']) {
      await createProject(page, uniqueName('look-names'), template)
      await expect(page.locator('.arch-node').first()).toBeVisible()
      // zoomed out far enough for the semantic zoom to draw the names large (the window decides where the fit ends, so zoom out until it does)
      for (let i = 0; i < 4 && (await page.locator('.canvas').getAttribute('data-lod')) !== 'compact'; i++) await page.locator('.zoom button[aria-label="Отдалить"]').click()
      await expect(page.locator('.canvas')).toHaveAttribute('data-lod', 'compact')
      await page.waitForTimeout(600)
      const cut = await page.evaluate(() => {
        const ctx = document.createElement('canvas').getContext('2d')!
        const out: string[] = []
        for (const el of document.querySelectorAll<HTMLElement>('.arch-node .name')) {
          const cs = getComputedStyle(el)
          ctx.font = `${cs.fontWeight} ${cs.fontSize} ${cs.fontFamily}`
          const room = el.clientWidth
          const wide = (el.textContent ?? '').trim().replace(/-/g, '- ').split(/\s+/).filter((w) => ctx.measureText(w).width > room + 0.5)
          if (room && wide.length) out.push(`${el.textContent} [${wide.join(', ')}] ${cs.fontSize} in ${Math.round(room)}`)
        }
        return out
      })
      expect(cut).toEqual([])
      await page.goto('/')
    }
  })

  test('a label that would sit on a card is not drawn over it: it waits for the pointer', async ({ page }) => {
    await createProject(page, uniqueName('look-labels'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await page.waitForTimeout(800)
    const overlaps = await page.evaluate(() => {
      const cards = Array.from(document.querySelectorAll('.arch-node')).map((e) => e.getBoundingClientRect())
      const hit = (a: DOMRect, b: DOMRect) => a.left < b.right - 1 && a.right > b.left + 1 && a.top < b.bottom - 1 && a.bottom > b.top + 1
      return Array.from(document.querySelectorAll('.edge-label:not(.flow)')).filter((l) => cards.some((c) => hit(l.getBoundingClientRect(), c))).map((l) => l.textContent)
    })
    expect(overlaps).toEqual([])
  })
})
