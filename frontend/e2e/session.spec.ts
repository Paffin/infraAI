import { expect, type BrowserContext, type Page } from '@playwright/test'
import { test } from './fixtures'
import { createProject, email, login, loginTo, password, uniqueName } from './helpers'

/**
 * The save path under the two things that take work away from a person: the connection going and the session ending (it is 12 hours long, so it ends under an
 * open editor as a matter of routine). Measured before the change, on a live stack:
 *   - offline: the banner said "ошибка сохранения: Failed to fetch" and sat on the middle of the toolbar; the connection coming back waited for the 3 s pause;
 *   - session: the first 401 is the check that runs a third of a second after an edit, the page went to /login with no word of why, and the edit was gone;
 *   - leaving the project while offline (the "Проекты" link) lost the edit without a word.
 * The edits are kept in this browser (`ia_unsaved:<user>:<project>`) and offered back; the copy is never dropped because the server moved on.
 */

const TEMPLATE = '2 ЦОД + witness'
const NAME_FIELD = '.inspector .prop input'

/** The keys of the copies of unsaved edits in the browser's storage. */
const copies = (page: Page) => page.evaluate(() => Object.keys(localStorage).filter((k) => k.startsWith('ia_unsaved:')))

async function openWithCard(page: Page, name: string) {
  await login(page)
  await createProject(page, uniqueName(name), TEMPLATE)
  await expect(page.locator('.arch-node').first()).toBeVisible()
  const id = page.url().split('/p/')[1]
  await page.locator('.arch-node').first().click()
  await expect(page.locator(NAME_FIELD).first()).toBeVisible()
  return id
}

test.describe('the connection goes', () => {
  test('the edit stays on the screen, the page says there is no connection in its own words and clear of the toolbar, and the connection coming back saves at once', async ({ page, context }) => {
    await openWithCard(page, 'offline')
    await context.setOffline(true)
    await page.locator(NAME_FIELD).first().fill('Edited offline')

    const strip = page.getByTestId('save-status')
    const notice = strip.locator('[data-failure="offline"]')
    await expect(notice).toBeVisible({ timeout: 10_000 })
    await expect(notice).toContainText(/нет связи/i)
    await expect(page.locator('.crumbs .meta')).toContainText('нет связи')
    expect(await page.locator('body').innerText(), 'the browser\'s own sentence is not for the page').not.toContain('Failed to fetch')
    await expect(page.locator('.arch-node', { hasText: 'Edited offline' })).toHaveCount(1) // the edit is on the screen

    // a strip of its own: above the work area, so it cannot sit on the toolbar; the toolbar takes clicks
    const bar = (await notice.boundingBox())!
    const toolbar = (await page.locator('.toolbar').boundingBox())!
    expect(bar.y + bar.height).toBeLessThanOrEqual(toolbar.y)
    await page.locator('.toolbar button').first().click({ trial: true })

    // the same on a narrow window, where the header has no room for words: the strip still says it
    await page.setViewportSize({ width: 1440, height: 900 })
    await expect(notice).toBeVisible()
    await expect(notice).toContainText(/нет связи/i)
    await expect(page.locator('.crumbs .st')).toHaveAttribute('title', /Нет связи с сервером/)

    // back online: saved within 2 s, while the first pause (3 s) is still running
    await context.setOffline(false)
    await expect(page.locator('.crumbs .meta')).toContainText('сохранено', { timeout: 2_000 })
    await expect(notice).toHaveCount(0)
  })

  test('leaving the project while the save cannot get through keeps the edit in this browser, says so, and offers it back', async ({ page, context }) => {
    const id = await openWithCard(page, 'leave')
    await context.setOffline(true)
    await page.locator(NAME_FIELD).first().fill('Typed before leaving')
    await page.locator('header.eh .crumbs a').click()
    await expect(page).toHaveURL(/\/$/)
    await expect(page.locator('.toast', { hasText: 'сохранены в этом браузере' })).toBeVisible()
    expect(await copies(page)).toHaveLength(1)

    await context.setOffline(false)
    await page.locator('article.project a[href="/p/' + id + '"]').first().click()
    const offer = page.getByTestId('unsaved-offer')
    await expect(offer).toBeVisible()
    await expect(offer).toContainText('Typed before leaving')
    await page.getByTestId('unsaved-restore').click()
    await expect(page.locator('.arch-node', { hasText: 'Typed before leaving' })).toHaveCount(1)
    await expect(page.locator('.crumbs .meta')).toContainText('сохранено', { timeout: 10_000 })
    await expect.poll(() => copies(page)).toEqual([])
    const saved = await (await page.request.get('/api/projects/' + id)).json()
    expect(saved.architecture.nodes.some((n: { name: string }) => n.name === 'Typed before leaving')).toBe(true)
  })

  test('a page that cannot reach the server while it starts says so in its own words, not the browser\'s', async ({ page }) => {
    await page.route('**/api/bootstrap', (route) => route.abort())
    await page.goto('/')
    await expect(page.locator('.boot-error')).toContainText('Нет связи с сервером')
    expect(await page.locator('body').innerText()).not.toContain('Failed to fetch')
  })
})

/** The editor on the screen and the other tab having stored a different change: the next save of this page is a real 409. Leaves the conflict on the screen. */
async function inConflict(page: Page, context: BrowserContext) {
  await login(page)
  await createProject(page, uniqueName('conflict'), TEMPLATE)
  const url = page.url()
  const id = url.split('/p/')[1]
  // the first visit lays the template out and stores it: the other tab must read the stored one, or its own first save would be the conflict
  await expect(page.locator('.crumbs .meta')).toContainText('сохранено', { timeout: 10_000 })

  const other = await context.newPage()
  await other.goto(url)
  await other.locator('.arch-node').first().click()
  await other.locator(NAME_FIELD).first().fill('From the other tab')
  await expect(other.locator('.crumbs .meta')).toContainText('сохранено', { timeout: 10_000 })
  await other.close()

  await page.locator('.arch-node').nth(2).click()
  await page.locator(NAME_FIELD).first().fill('Mine in conflict')
  const conflict = page.locator('.conflict')
  await expect(conflict).toBeVisible({ timeout: 10_000 })
  return { id, conflict }
}

test.describe('the session ends', () => {
  test('under an open editor the edit is kept, the sign-in page says why, and after signing in the edit is offered and restored', async ({ page, context }) => {
    const id = await openWithCard(page, 'expiry')
    await context.clearCookies() // the session is gone on the server's side as far as the next request is concerned
    await page.locator(NAME_FIELD).first().fill('Typed as the session ended')

    await expect(page).toHaveURL(/\/login\?.*expired=kept/)
    expect(new URL(page.url()).searchParams.get('next')).toBe('/p/' + id)
    const said = page.getByTestId('login-expired')
    await expect(said).toContainText('Сессия закончилась')
    await expect(said).toContainText('сохранены в этом браузере')
    const keys = await copies(page)
    expect(keys).toHaveLength(1)
    expect(keys[0]).toMatch(new RegExp(`^ia_unsaved:[0-9a-f-]+:${id}$`))

    await loginTo(page, /\/p\//)
    const offer = page.getByTestId('unsaved-offer')
    await expect(offer).toBeVisible()
    await expect(offer).toContainText('Typed as the session ended') // it names the card that was changed
    await expect(offer.locator('.sb-moved')).toHaveCount(0) // nobody else touched the project
    await expect(page.locator('.arch-node', { hasText: 'Typed as the session ended' })).toHaveCount(0) // the server never had it

    await page.getByTestId('unsaved-restore').click()
    await expect(page.locator('.arch-node', { hasText: 'Typed as the session ended' })).toHaveCount(1)
    await expect(page.locator('.crumbs .meta')).toContainText('сохранено', { timeout: 10_000 })
    await expect(offer).toHaveCount(0)
    await expect.poll(() => copies(page)).toEqual([])
    await page.reload()
    await expect(page.locator('.arch-node', { hasText: 'Typed as the session ended' })).toHaveCount(1) // and the server has it now
  })

  test('when somebody else saved meanwhile the offer warns, nothing on the server changes until the danger question is answered, and the edits can become a project of their own', async ({ page, context, playwright }) => {
    const id = await openWithCard(page, 'moved')
    await context.clearCookies()
    await page.locator(NAME_FIELD).first().fill('Mine')
    await expect(page).toHaveURL(/\/login\?.*expired=kept/)

    // another browser saves a different change to the same project
    const other = await playwright.request.newContext({ baseURL: test.info().project.use.baseURL })
    expect((await other.post('/api/auth/login', { data: { email, password } })).ok()).toBe(true)
    const cur = await (await other.get('/api/projects/' + id)).json()
    const theirs = JSON.parse(JSON.stringify(cur.architecture))
    theirs.nodes[theirs.nodes.length - 1].name = 'Theirs'
    expect((await other.put(`/api/projects/${id}/draft`, { data: { architecture: theirs, expectedRev: cur.rev } })).ok()).toBe(true)

    await loginTo(page, /\/p\//)
    const offer = page.getByTestId('unsaved-offer')
    await expect(offer).toBeVisible()
    await expect(offer.locator('.sb-moved')).toBeVisible()
    await expect(page.locator('.arch-node', { hasText: 'Theirs' })).toHaveCount(1)

    // Restore asks first; "Cancel" changes nothing, on the screen or on the server
    await page.getByTestId('unsaved-restore').click()
    const ask = page.locator('.modal', { hasText: 'Заменить схему на сервере?' })
    await expect(ask).toBeVisible()
    await ask.getByRole('button', { name: 'Отмена' }).click()
    await expect(page.locator('.arch-node', { hasText: 'Theirs' })).toHaveCount(1)
    const onServer = await (await other.get('/api/projects/' + id)).json()
    expect(onServer.architecture.nodes.some((n: { name: string }) => n.name === 'Mine')).toBe(false)
    expect(onServer.architecture.nodes.some((n: { name: string }) => n.name === 'Theirs')).toBe(true)
    expect(await copies(page)).toHaveLength(1) // and the copy is still there

    // "Open as a new project": a project that holds the edits, the other one untouched
    await page.getByTestId('unsaved-new').click()
    await expect(page).toHaveURL(new RegExp('/p/(?!' + id + ')'))
    await expect(page.locator('.arch-node', { hasText: 'Mine' })).toHaveCount(1)
    await expect(page.locator('.arch-node', { hasText: 'Theirs' })).toHaveCount(0)
    await expect(page.getByTestId('unsaved-offer')).toHaveCount(0)
    await expect.poll(() => copies(page)).toEqual([])
    const original = await (await other.get('/api/projects/' + id)).json()
    expect(original.architecture.nodes.some((n: { name: string }) => n.name === 'Theirs')).toBe(true)
    await other.dispose()
  })

  test('signing out asks first when copies of unsaved edits are in the browser, names how many, and removes them when the answer is yes', async ({ page, context }) => {
    await openWithCard(page, 'signout')
    await context.setOffline(true)
    await page.locator(NAME_FIELD).first().fill('Typed before signing out')
    await page.locator('header.eh .crumbs a').click()
    await expect(page).toHaveURL(/\/$/)
    await expect.poll(() => copies(page)).toHaveLength(1)
    await context.setOffline(false)

    await page.getByRole('button', { name: 'Выйти' }).click()
    const ask = page.locator('.modal', { hasText: 'Выйти и удалить несохранённые правки?' })
    await expect(ask).toBeVisible()
    await expect(ask).toContainText('в этом браузере: 1.')
    await ask.getByRole('button', { name: 'Отмена' }).click()
    await expect(ask).toHaveCount(0)
    await expect(page).not.toHaveURL(/\/login/) // still signed in, and the copy is where it was
    expect(await copies(page)).toHaveLength(1)

    await page.getByRole('button', { name: 'Выйти' }).click()
    await ask.getByRole('button', { name: 'Выйти и удалить' }).click()
    await expect(page).toHaveURL(/\/login/)
    expect(await copies(page)).toEqual([])
  })
})

test.describe('a conflict', () => {
  test('can be answered by opening the edits as a project of their own, the other version untouched', async ({ page, context }) => {
    const { id, conflict } = await inConflict(page, context)
    // a strip, not a card over the diagram
    const bar = (await conflict.boundingBox())!
    expect((await page.locator('.toolbar').boundingBox())!.y).toBeGreaterThanOrEqual(bar.y + bar.height)
    // and the tooltip of the header says it in the words of the page, not with the sentence the server sent (English, in a Russian page)
    await expect(page.locator('.crumbs .st')).toHaveAttribute('title', 'конфликт версий')

    await page.getByTestId('conflict-new').click()
    await expect(page).toHaveURL(new RegExp('/p/(?!' + id + ')'))
    await expect(page.locator('.arch-node', { hasText: 'Mine in conflict' })).toHaveCount(1)
    await expect(page.locator('.conflict')).toHaveCount(0)
    const original = await (await page.request.get('/api/projects/' + id)).json()
    expect(original.architecture.nodes.some((n: { name: string }) => n.name === 'From the other tab')).toBe(true)
    expect(original.architecture.nodes.some((n: { name: string }) => n.name === 'Mine in conflict')).toBe(false)
  })

  test('that is left keeps the edit in this browser, and the offer does not blame a lost connection or a session that did not end', async ({ page, context }) => {
    const { id } = await inConflict(page, context)
    await page.locator('header.eh .crumbs a').click()
    await expect(page).toHaveURL(/\/$/)
    await expect(page.locator('.toast', { hasText: 'сохранены в этом браузере' })).toBeVisible()
    expect(await copies(page)).toHaveLength(1)

    await page.locator('article.project a[href="/p/' + id + '"]').first().click()
    const offer = page.getByTestId('unsaved-offer')
    await expect(offer).toBeVisible()
    await expect(offer).toContainText('Mine in conflict')
    await expect(offer.locator('.sb-moved')).toBeVisible() // the other tab saved since
    const said = await offer.innerText()
    expect(said).toContain('Они не были сохранены на сервере')
    expect(said).not.toMatch(/сессия|связь/i)
  })

  test('under a session that ends is kept too (a real 409 first, then the cookie goes)', async ({ page, context }) => {
    await inConflict(page, context)
    await context.clearCookies()
    await page.locator(NAME_FIELD).first().fill('Typed after the conflict') // the check that follows every edit answers 401
    await expect(page).toHaveURL(/\/login\?.*expired=kept/)
    expect(await copies(page)).toHaveLength(1)

    await loginTo(page, /\/p\//)
    const offer = page.getByTestId('unsaved-offer')
    await expect(offer).toBeVisible()
    await expect(offer).toContainText('Typed after the conflict')
    await expect(offer.locator('.sb-moved')).toBeVisible()
  })
})

test.describe('the offer of a project', () => {
  /** The router of the page, to move to another project the way a link does: the editor stays mounted and the route changes under it. */
  const goTo = (page: Page, id: string) =>
    page.evaluate((to) => {
      const app = document.getElementById('app') as unknown as { __vue_app__: { config: { globalProperties: { $router: { push: (to: string) => Promise<unknown> } } } } }
      return app.__vue_app__.config.globalProperties.$router.push('/p/' + to)
    }, id)

  test('is not shown beside the load of another project, or the error of a load that failed, and the other project is not touched', async ({ page, context }) => {
    const a = await openWithCard(page, 'offer-a')
    await context.setOffline(true)
    await page.locator(NAME_FIELD).first().fill('Edited in A')
    await page.locator('header.eh .crumbs a').click()
    await expect(page).toHaveURL(/\/$/)
    expect(await copies(page)).toHaveLength(1)
    await context.setOffline(false)

    await createProject(page, uniqueName('offer-b'), TEMPLATE)
    const b = page.url().split('/p/')[1]
    await expect(page.locator('.crumbs .meta')).toContainText('сохранено', { timeout: 10_000 })
    await page.locator('header.eh .crumbs a').click()
    await expect(page).toHaveURL(/\/$/)
    await page.locator('article.project a[href="/p/' + a + '"]').first().click()
    const offer = page.getByTestId('unsaved-offer')
    await expect(offer).toBeVisible()

    // B answers late: its skeleton is on the screen, and the edits of A are not offered beside it
    let release!: () => void
    const held = new Promise<void>((r) => { release = r })
    await page.route('**/api/projects/' + b, async (route) => {
      if (route.request().method() === 'GET') await held
      await route.continue()
    })
    await goTo(page, b)
    await expect(page.getByTestId('editor-skeleton')).toBeVisible()
    await expect(offer).toHaveCount(0)
    release()
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await expect(offer).toHaveCount(0)
    await expect(page.locator('.conflict')).toHaveCount(0) // and B was not taken for a project somebody else changed
    await page.unroute('**/api/projects/' + b)

    // back to A: the offer is there again (the copy was never touched); B cannot be loaded: the error is B's, and A's offer is not beside it
    await goTo(page, a)
    await expect(offer).toBeVisible()
    await page.route('**/api/projects/' + b, (route) => route.abort())
    await goTo(page, b)
    await expect(page.locator('.state .banner-err')).toContainText('Нет связи с сервером')
    await expect(offer).toHaveCount(0)
    expect(await copies(page)).toHaveLength(1)
    await page.unroute('**/api/projects/' + b)
  })

  test('lists as many changes as the setting says (`editor.unsavedOfferLines`) and counts the rest', async ({ page }) => {
    const id = await openWithCard(page, 'lines')
    await expect(page.locator('.crumbs .meta')).toContainText('сохранено', { timeout: 10_000 })
    const userId = (await (await page.request.get('/api/bootstrap')).json()).user.id as string
    const cur = await (await page.request.get('/api/projects/' + id)).json()
    const edited = JSON.parse(JSON.stringify(cur.architecture))
    expect(edited.nodes.length).toBeGreaterThanOrEqual(6)
    edited.nodes.slice(0, 6).forEach((n: { name: string }, i: number) => { n.name = 'Renamed ' + i })
    await page.evaluate(([key, value]) => localStorage.setItem(key, value), [`ia_unsaved:${userId}:${id}`, JSON.stringify({ v: 1, userId, projectId: id, baseRev: cur.rev, savedAt: Date.now(), architecture: edited })])

    const offer = page.getByTestId('unsaved-offer')
    const lines = async (shown: number, rest: number) => {
      await expect(offer.locator('.sb-lines li:not(.more)')).toHaveCount(shown)
      await expect(offer.locator('.sb-lines .more')).toContainText(`ещё ${rest}`)
    }
    await page.reload()
    await lines(4, 2) // the default of the registry

    const setting = (value: number) => page.request.put('/api/admin/settings', { data: { changes: { 'editor.unsavedOfferLines': value } } })
    try {
      expect((await setting(2)).ok()).toBe(true)
      await page.reload()
      await lines(2, 4)
    } finally {
      expect((await setting(4)).ok()).toBe(true) // put it back, whatever happened above
    }
  })
})
