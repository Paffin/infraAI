import { expect, type Page } from '@playwright/test'
import { test } from './fixtures'
import { createProject, login, uniqueName } from './helpers'

// The library holds hundreds of products: what a person is not looking for folds away and stays folded, a search opens everything it finds,
// and a product's name is read in full.

const section = (page: Page, id: string) => page.locator('.library section', { has: page.getByTestId(`lib-cat-${id}`) })

test.describe('the library', () => {
  test.beforeEach(async ({ page }) => {
    await login(page)
    await createProject(page, uniqueName('library'), 'Event-driven')
    await expect(page.locator('.arch-node').first()).toBeVisible()
  })

  test('a category folds and stays folded after a reload; a search opens what it finds', async ({ page }) => {
    const head = page.getByTestId('lib-cat-data')
    const data = section(page, 'data')
    await expect(head).toHaveAttribute('aria-expanded', 'true')
    await expect(data.locator('.kind').first()).toBeVisible()
    // the heading says how many products the category holds
    expect(Number(await head.locator('.found').innerText())).toBeGreaterThan(50)

    await head.click()
    await expect(head).toHaveAttribute('aria-expanded', 'false')
    await expect(data.locator('.kind')).toHaveCount(0)
    // the other categories are where they were
    await expect(section(page, 'edge').locator('.kind').first()).toBeVisible()

    // it is the person's choice, kept in the browser
    await page.reload()
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await expect(page.getByTestId('lib-cat-data')).toHaveAttribute('aria-expanded', 'false')

    // a search looks everywhere: the folded category opens for what it holds, and folds again when the search is cleared
    const search = page.locator('.library .search input')
    await search.fill('postgresql')
    await expect(page.getByTestId('lib-cat-data')).toHaveAttribute('aria-expanded', 'true')
    await expect(page.locator('.library .kind[data-kind="postgresql"]')).toBeVisible()
    await search.fill('')
    await expect(page.getByTestId('lib-cat-data')).toHaveAttribute('aria-expanded', 'false')

    // "expand all" brings everything back
    await page.locator('.library .lnk', { hasText: 'Развернуть всё' }).click()
    await expect(page.getByTestId('lib-cat-data')).toHaveAttribute('aria-expanded', 'true')
    await expect(data.locator('.kind').first()).toBeVisible()
  })

  test('the reference architectures fold like a category, and are open for a new project', async ({ page }) => {
    const head = page.getByTestId('lib-cat-templates')
    const tpl = page.locator('.library .tpl')
    await expect(head).toHaveAttribute('aria-expanded', 'true')
    expect(await tpl.count()).toBeGreaterThan(5)
    await head.click()
    await expect(head).toHaveAttribute('aria-expanded', 'false')
    await expect(tpl).toHaveCount(0)
    await head.click()
    expect(await tpl.count()).toBeGreaterThan(5)
  })

  test('a long product name is read in full: the tile takes the width it needs and wraps the name', async ({ page }) => {
    await page.locator('.library .search input').fill('kaspersky')
    const tile = page.locator('.library .kind[data-kind="kuma"]')
    await expect(tile).toBeVisible()
    const name = tile.locator('.nm')
    await expect(name).toHaveText('Kaspersky Unified Monitoring and Analysis Platform (KUMA)')
    // nothing of it is cut off: the text fits the lines the tile gives it
    expect(await name.evaluate((e) => e.scrollHeight <= e.clientHeight + 1)).toBe(true)
    // one column in a panel of this width: the tile is the width of the panel, not half of it
    const [tileBox, panelBox] = await Promise.all([tile.boundingBox(), page.locator('.library').boundingBox()])
    expect(tileBox!.width).toBeGreaterThan(panelBox!.width * 0.7)
  })
})
