import { expect } from '@playwright/test'
import { madeByApi, test } from './fixtures'
import { login, uniqueName } from './helpers'

// The server pages the list of projects (500 at most in a page, `next` points at the rest) and the page reads it to the end: the cards search and sort over
// all of them. Before, the list stopped at the 500th project and the 501st did not exist for the interface.

test.describe('the list of projects past one page', () => {
  test('a project beyond the first 500 is on the screen, and so is its full description when it is copied', async ({ page }) => {
    test.setTimeout(300_000)
    await login(page)
    const settings = (await (await page.request.get('/api/admin/settings')).json()).settings as { key: string; value: unknown }[]
    const was = settings.find((s) => s.key === 'limits.maxProjectsPerUser')!.value
    const mark = uniqueName('page')
    const total = 510
    try {
      // the limit of projects per user is lifted for this test only
      expect((await page.request.put('/api/admin/settings', { data: { changes: { 'limits.maxProjectsPerUser': 2000 } } })).ok()).toBe(true)
      const description = 'long description '.repeat(60) // 960 characters: past what a card carries (limits.listDescriptionChars)
      // one after another: creating a project evaluates it, which is a heavy request, and a person may have a few of those at once (`limits.heavyConcurrentPerUser`)
      for (let i = 0; i < total; i++) {
        const r = await page.request.post('/api/projects', { data: { name: `${mark}-${i}`, description } })
        expect(r.ok(), await r.text()).toBe(true)
        madeByApi.push((await r.json()).id)
      }

      const pages: string[] = []
      page.on('request', (req) => { if (req.method() === 'GET' && /\/api\/projects(\?|$)/.test(req.url())) pages.push(req.url()) })
      await page.goto('/')
      await page.fill('.search input', mark)
      await expect(page.locator('article.project')).toHaveCount(total, { timeout: 60_000 })
      expect(pages.length, 'the first page has no query string, the others have the cursor').toBeGreaterThanOrEqual(2)
      expect(pages[0]).toMatch(/\/api\/projects$/)
      expect(pages.slice(1).every((u) => u.includes('cursor='))).toBe(true)

      // the card carries the beginning of the description; the copy has all of it
      const card = page.locator('article.project', { hasText: `${mark}-0` }).first()
      await card.getByRole('button', { name: 'Дублировать' }).click()
      await page.fill('.modal input', `${mark}-copy`)
      await page.locator('.modal-foot .btn-primary').click()
      await expect(page.locator('article.project', { hasText: `${mark}-copy` })).toBeVisible({ timeout: 30_000 })
      const copy = (await (await page.request.get('/api/projects?limit=5')).json()).projects.find((p: { name: string }) => p.name === `${mark}-copy`)
      expect(copy, 'the copy is among the newest').toBeTruthy()
      const whole = (await (await page.request.get(`/api/projects/${copy.id}`)).json()).project.description as string
      expect(whole.length).toBe(description.length)
      madeByApi.push(copy.id)
    } finally {
      await page.request.put('/api/admin/settings', { data: { changes: { 'limits.maxProjectsPerUser': was } } })
    }
  })
})
