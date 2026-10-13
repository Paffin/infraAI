import { expect, type Page } from '@playwright/test'
import { test } from './fixtures'
import { createProject, login, uniqueName } from './helpers'

const node = (page: Page, text: string | RegExp) => page.locator('.arch-node', { hasText: text }).first()

test.describe('products, variants and versions', () => {
  test.beforeEach(async ({ page }) => {
    await login(page)
    await createProject(page, uniqueName('product'), 'Event-driven')
  })

  test('the library finds a product by the way it is deployed and adds it that way', async ({ page }) => {
    const search = page.locator('.library .search input')
    await search.fill('aurora')
    // the products that have a variant by that name, with the variant named under them
    await expect(page.locator('.library .kind[data-kind="postgresql"]')).toContainText('Amazon Aurora PostgreSQL')
    await expect(page.locator('.library .kind[data-kind="mysql"]')).toContainText('Amazon Aurora MySQL')
    await page.locator('.library .kind[data-kind="mysql"]').click()
    await expect(page.locator('.inspector')).toBeVisible()
    // the node is a managed MySQL: the variant is chosen, and the card carries the provider's cloud
    await expect(page.locator('.inspector select[data-prop="variant"]')).toHaveValue('aurora')
    await expect(node(page, 'MySQL').locator('.mgd')).toBeVisible()
    await search.fill('')
  })

  test('functions group the products that replace each other', async ({ page }) => {
    await page.locator('.library .lnk', { hasText: 'Развернуть всё' }).click()
    const rdbms = page.locator('.library .fn', { has: page.locator('.fh', { hasText: 'Реляционные СУБД' }) })
    for (const id of ['postgresql', 'mysql', 'mariadb', 'mssql', 'oracle']) await expect(rdbms.locator(`.kind[data-kind="${id}"]`)).toBeVisible()
    // the heading counts the products of the function: that many tiles, the usual choices first
    const total = Number(await rdbms.locator('.fh .cnt').innerText())
    await expect(rdbms.locator('.kind:not(.more-tile)')).toHaveCount(total)
    expect(total).toBeGreaterThan(5)
    await expect(rdbms.locator('.kind').first()).toHaveAttribute('data-kind', 'postgresql')
    await page.locator('.library .lnk', { hasText: 'Свернуть всё' }).click()
    // folded: the usual choices stay at hand and the rest is one click away
    await expect(rdbms.locator('.kind:not(.more-tile)')).toHaveCount(2)
    await rdbms.locator('.more-tile').click()
    await expect(rdbms.locator('.kind:not(.more-tile)')).toHaveCount(total)
  })

  test('an old version is judged by the release table and the recommended line fixes it', async ({ page }) => {
    await node(page, /PostgreSQL/).click()
    const version = page.locator('.vf[data-prop="version"]')
    await version.locator('input').fill('12.4')
    await page.keyboard.press('Escape')
    // support of the 12 line ended in November 2024: the chip says so, the table names the line it recommends
    await expect(version.locator('.tag-crit')).toContainText('Поддержка закончилась')
    await expect(version.locator('.reco')).toContainText('Рекомендуется')
    // the rule engine says the same in the checks panel
    await expect(page.locator('.checks, .right').getByText(/PostgreSQL 12\.4/).first()).toBeVisible({ timeout: 20_000 })
    await version.locator('.use').click()
    await expect(version.locator('input')).not.toHaveValue('12.4')
    await expect(version.locator('.tag-ok')).toBeVisible()
    await expect(version.locator('.tag-crit')).toHaveCount(0)
    // the list of release lines shows every line with its state
    await version.locator('input').click()
    await expect(version.locator('.list .opt').first()).toBeVisible()
    expect(await version.locator('.list .opt').count()).toBeGreaterThan(4)
  })

  test('switching the product keeps what fits and says what went', async ({ page }) => {
    await node(page, /PostgreSQL/).click()
    await page.locator('.vf[data-prop="version"] input').fill('16.3')
    await page.locator('.inspector select').filter({ has: page.locator('optgroup') }).selectOption('mysql')
    await expect(page.locator('.toast, .toasts')).toContainText('version')
    await expect(page.locator('.vf[data-prop="version"] input')).toHaveValue('')
    await expect(page.locator('.inspector select').first()).toBeVisible()
    // the variant list is MySQL's now
    await expect(page.locator('.inspector select[data-prop="variant"] option', { hasText: 'InnoDB Cluster' })).toHaveCount(1)
  })

  test('the wizard builds the platform from the product and variant picked per layer', async ({ page }) => {
    await page.goto('/')
    await page.getByRole('button', { name: 'Новый проект' }).first().click()
    await page.locator('.wizard-card').click()
    await expect(page.locator('.wiz .site').first()).toBeVisible({ timeout: 20_000 })
    const db = page.locator('.wiz .layer', { hasText: 'База данных' })
    await db.locator('select').first().selectOption('mysql')
    await db.locator('select').nth(1).selectOption({ label: '☁ Amazon Aurora MySQL' })
    // a managed database holds its own quorum: the DCS layer is not needed any more
    await expect(page.locator('.wiz .layer', { hasText: 'Консенсус' }).locator('.unused')).toBeVisible({ timeout: 20_000 })
    await page.locator('.wiz .cfg input.input').first().fill(uniqueName('aurora'))
    await page.locator('.modal-foot .btn-primary').click()
    await page.waitForSelector('.canvas')
    await expect(page.locator('.arch-node', { hasText: /MySQL|База/ }).first()).toBeVisible()
    await expect(page.locator('.arch-node .mgd').first()).toBeVisible()
  })
})

test('the admin panel shows how fresh the release tables are', async ({ page }) => {
  await login(page)
  await page.goto('/admin/catalog/versions')
  await expect(page.locator('.vstat')).toContainText('Таблиц выпусков')
  await expect(page.locator('.items-table tbody tr').first()).toBeVisible()
  await page.locator('input[type="search"]').fill('postgresql')
  await expect(page.locator('.items-table tbody tr').first()).toContainText('PostgreSQL')
})
