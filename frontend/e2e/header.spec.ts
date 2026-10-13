import { expect } from '@playwright/test'
import { test } from './fixtures'
import { createProject, login, uniqueName } from './helpers'

/**
 * The editor header carries a dozen things; it is as wide as the window, and every window width people actually use
 * must show all of them without one lying over another or running off the edge. What does not fit gives up its words
 * (see the breakpoints in `EditorHeader.vue`) - the project name is the last to go.
 */
test.describe('the editor header at every common window width', () => {
  for (const width of [1280, 1366, 1440, 1460, 1536, 1600, 1680, 1700, 1740, 1790, 1810, 1920, 1960, 2090, 2110, 2560]) {
    test(`${width}px: nothing overlaps, nothing runs off the edge, the project name is readable`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 })
      await login(page)
      await createProject(page, uniqueName('header'), '2 ЦОД + witness')
      await expect(page.locator('.arch-node').first()).toBeVisible()
      const g = await page.evaluate(() => {
        const eh = document.querySelector('.eh') as HTMLElement
        const kids = Array.from(eh.children)
          .map((c) => ({ cls: String((c as HTMLElement).className || c.tagName), r: c.getBoundingClientRect() }))
          .filter((k) => k.r.width > 0 && !k.cls.includes('spacer'))
          .map((k) => ({ cls: k.cls, l: k.r.left, r: k.r.right }))
        return { scroll: eh.scrollWidth, client: eh.clientWidth, kids, name: (document.querySelector('.eh .pname') as HTMLElement).getBoundingClientRect().width, viewport: window.innerWidth }
      })
      expect(g.scroll, 'the header does not scroll sideways').toBeLessThanOrEqual(g.client + 1)
      for (let i = 1; i < g.kids.length; i++) expect(g.kids[i].l, `${g.kids[i].cls} starts after ${g.kids[i - 1].cls} ends`).toBeGreaterThanOrEqual(g.kids[i - 1].r - 1)
      expect(g.kids[g.kids.length - 1].r, 'the last control is inside the window').toBeLessThanOrEqual(g.viewport)
      expect(g.name, 'the project name has room').toBeGreaterThanOrEqual(40)
    })
  }
})
