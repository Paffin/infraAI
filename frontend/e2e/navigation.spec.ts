import { expect, type Page } from '@playwright/test'
import { madeByApi, test } from './fixtures'
import { createProject, login, settled, uniqueName } from './helpers'

/**
 * Moving between pages, with the keyboard and with the browser: a project card is a link (it opens with Enter, in a new tab, from the context menu), every page has
 * a title of its own (tabs are told apart), and a new page takes the keyboard (the focus goes to its heading, which a screen reader reads).
 *
 * Measured before the change: `article.project` held 0 links and 3 Tab stops (the icon buttons), Ctrl+click on a card opened nothing and navigated the same tab, and
 * `document.title` was "Infra Architect" on every page.
 */
const TEMPLATE = '2 ЦОД + witness'

async function appName(page: Page): Promise<string> {
  return (await (await page.request.get('/api/bootstrap')).json()).app.name
}

/** A project made through the API (cleaned up by the fixture), so that the list has a card with a known name. */
async function makeProject(page: Page, name: string): Promise<string> {
  const r = await page.request.post('/api/projects', { data: { name } })
  expect(r.ok()).toBe(true)
  const id = (await r.json()).id as string
  madeByApi.push(id)
  return id
}

test.describe('project cards are links', () => {
  test('every card holds a link; Tab reaches it and Enter opens the project', async ({ page }) => {
    await login(page)
    const name = uniqueName('nav')
    const id = await makeProject(page, name)
    await page.reload()
    const card = page.locator('article.project', { hasText: name })
    const link = card.locator('a[href]').first()
    await expect(link).toHaveAttribute('href', `/p/${id}`)

    const cards = await page.locator('article.project').evaluateAll((els) => els.map((e) => e.querySelectorAll('a[href]').length))
    expect(cards.length).toBeGreaterThan(0)
    expect(cards.filter((n) => n === 0), 'cards without a link').toHaveLength(0)

    // from the search box: the next stops are the sort, the button for a new project and then the cards, the newest first
    await page.locator('.search input').focus()
    for (let i = 0; i < 14 && !(await link.evaluate((el) => el === document.activeElement)); i++) await page.keyboard.press('Tab')
    await expect(link).toBeFocused()
    await page.keyboard.press('Enter')
    await expect(page).toHaveURL(new RegExp(`/p/${id}$`))
  })

  test('the focused card has a ring round the whole card', async ({ page }) => {
    await login(page)
    const name = uniqueName('ring')
    await makeProject(page, name)
    await page.reload()
    const card = page.locator('article.project', { hasText: name })
    await card.locator('a[href]').first().focus()
    await page.keyboard.press('Shift+Tab') // out and back in, by the keyboard: the ring shows for the keyboard
    await page.keyboard.press('Tab')
    const outline = await card.evaluate((el) => getComputedStyle(el).outlineStyle)
    expect(outline).not.toBe('none')
  })

  test('Ctrl+click opens the project in a second page and leaves the list where it is', async ({ page, context }) => {
    await login(page)
    const name = uniqueName('tabs')
    const id = await makeProject(page, name)
    await page.reload()
    const card = page.locator('article.project', { hasText: name })
    // on the name, and on the card away from the name: both are the link
    for (const target of [{ at: card.locator('a[href]').first(), pos: undefined }, { at: card, pos: { x: 30, y: 30 } }]) {
      const [opened] = await Promise.all([context.waitForEvent('page'), target.at.click({ modifiers: ['Control'], position: target.pos })])
      await opened.waitForLoadState('domcontentloaded')
      expect(opened.url()).toMatch(new RegExp(`/p/${id}$`))
      expect(page.url()).toMatch(/\/$/)
      await opened.close()
    }
  })

  test('a plain click anywhere on the card opens it, as it always did', async ({ page }) => {
    await login(page)
    const name = uniqueName('click')
    const id = await makeProject(page, name)
    await page.reload()
    await page.locator('article.project', { hasText: name }).click({ position: { x: 30, y: 30 } })
    await expect(page).toHaveURL(new RegExp(`/p/${id}$`))
  })

  test('the buttons of the card are above the link: pressing one does not open the project', async ({ page }) => {
    await login(page)
    const name = uniqueName('buttons')
    await makeProject(page, name)
    await page.reload()
    const card = page.locator('article.project', { hasText: name })
    for (const button of ['Переименовать', 'Удалить']) {
      await card.getByRole('button', { name: button }).click()
      await expect(page.locator('[role="dialog"][aria-modal="true"]')).toHaveCount(1)
      expect(new URL(page.url()).pathname, `${button} must not open the project`).toBe('/')
      await page.keyboard.press('Escape')
      await expect(page.locator('[role="dialog"][aria-modal="true"]')).toHaveCount(0)
    }
  })

  test('deleting a card hands the keyboard to the card that took its place', async ({ page }) => {
    await login(page)
    const name = uniqueName('del')
    await makeProject(page, name)
    await page.reload()
    const card = page.locator('article.project', { hasText: name })
    await card.getByRole('button', { name: 'Удалить' }).focus()
    await page.keyboard.press('Enter')
    await page.locator('.modal-foot .btn-danger').click()
    await expect(card).toHaveCount(0)
    const where = await page.evaluate(() => ({ inCard: !!document.activeElement?.closest('article.project'), tag: document.activeElement?.tagName }))
    expect(where.inCard || where.tag === 'H1', 'the focus is on a card or on the heading, not on the body').toBe(true)
  })
})

test.describe('every page has its title', () => {
  test('the sign-in page, the list and the editor: the page, then the service; the editor follows a rename', async ({ page }) => {
    await page.goto('/login')
    const app = (await (await page.request.get('/api/public/config')).json()).appName as string
    await expect(page).toHaveTitle(`Вход · ${app}`)

    await login(page)
    await expect(page).toHaveTitle(`Проекты · ${app}`)

    const name = uniqueName('title')
    await createProject(page, name, TEMPLATE)
    await expect(page).toHaveTitle(`${name} · ${app}`)

    const renamed = uniqueName('renamed')
    await page.locator('.pname').click()
    await page.fill('.modal input.input', renamed)
    await page.locator('.modal-foot .btn-primary').click()
    await expect(page).toHaveTitle(`${renamed} · ${app}`)

    // the language does not touch a project's name, and a reload of the bootstrap does not put the service's name over it
    await page.getByRole('button', { name: 'EN', exact: true }).click()
    await expect(page).toHaveTitle(`${renamed} · ${app}`)
    await page.reload()
    await expect(page).toHaveTitle(`${renamed} · ${app}`)

    await page.locator('header.eh .crumbs a').click()
    await expect(page).toHaveTitle(`Projects · ${app}`)
    await page.getByRole('button', { name: 'RU', exact: true }).click()
    await expect(page).toHaveTitle(`Проекты · ${app}`)
  })

  test('the title is the one of the project that is open, not the one that was open before', async ({ page }) => {
    await login(page)
    const app = await appName(page)
    const first = uniqueName('first')
    const second = uniqueName('second')
    await makeProject(page, first)
    const secondId = await makeProject(page, second)
    await page.goto(`/p/${secondId}`)
    await expect(page).toHaveTitle(`${second} · ${app}`)
    await page.locator('header.eh .crumbs a').click()
    await expect(page).toHaveTitle(`Проекты · ${app}`)
    await page.locator('article.project', { hasText: first }).locator('a[href]').first().click()
    await expect(page).toHaveTitle(`${first} · ${app}`)
  })

  test('the admin panel: each of its 20 pages is named, and no two are alike', async ({ page }) => {
    await login(page)
    const app = await appName(page)
    const pages = [
      ...['settings', 'llm', 'rules', 'templates', 'iac', 'prompts', 'engine', 'backup', 'users', 'audit', 'system'],
      ...['kind', 'edge_type', 'zone_type', 'category', 'versions', 'common_props'].map((s) => `catalog/${s}`),
      ...['pattern', 'stack', 'settings'].map((s) => `placement/${s}`),
    ]
    expect(pages).toHaveLength(20)
    const titles: string[] = []
    for (const p of pages) {
      await page.goto(`/admin/${p}`)
      await expect(page, p).toHaveTitle(/^Администрирование · /)
      const title = await page.title()
      expect(title, p).toMatch(new RegExp(`^Администрирование · .+ · ${app.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}$`))
      titles.push(title)
    }
    expect(new Set(titles).size, titles.join('\n')).toBe(pages.length)
    // /admin alone is the settings, as the page itself shows
    await page.goto('/admin')
    await expect(page).toHaveTitle(titles[0])
  })
})

test.describe('a new page takes the keyboard', () => {
  test('from the list to a project the focus is on the project\'s heading, and back on the list\'s', async ({ page }) => {
    await login(page)
    const name = uniqueName('focus')
    const id = await makeProject(page, name)
    await page.reload()
    await page.locator('article.project', { hasText: name }).locator('a[href]').first().click()
    await expect(page).toHaveURL(new RegExp(`/p/${id}$`))
    // the heading of the editor is the project's name: it is there when the project is loaded
    await expect(page.locator('header.eh h1')).toContainText(name)
    await expect(page.locator('header.eh h1')).toBeFocused()

    await page.goBack()
    await expect(page.locator('main h1')).toBeFocused()
  })

  test('a move inside the admin panel leaves the focus on the link that was pressed', async ({ page }) => {
    await login(page)
    await page.goto('/admin/settings')
    const link = page.locator('.subnav a[href="/admin/users"]')
    await link.focus()
    await page.keyboard.press('Enter')
    await expect(page).toHaveURL(/\/admin\/users$/)
    await settled(page)
    await expect(link).toBeFocused()
  })

  test('a change of the address that is not a new page (the query) does not move the focus', async ({ page }) => {
    await login(page)
    await createProject(page, uniqueName('query'), TEMPLATE)
    await expect(page.locator('header.eh h1')).toBeFocused()
    const score = page.locator('header.eh .score')
    await score.focus()
    await page.evaluate(() => {
      const app = document.getElementById('app') as unknown as { __vue_app__: { config: { globalProperties: { $router: { replace: (to: unknown) => void } } } } }
      app.__vue_app__.config.globalProperties.$router.replace({ query: { probe: '1' } })
    })
    await expect(page).toHaveURL(/probe=1/)
    await settled(page)
    await expect(score).toBeFocused()
  })
})
