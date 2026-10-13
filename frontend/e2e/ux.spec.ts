import { expect, type Page } from '@playwright/test'
import { test } from './fixtures'
import { createProject, login, uniqueName, openAnalysis } from './helpers'

const TEMPLATE = '2 ЦОД + witness'

const tab = (page: Page, name: string) => page.locator('.right .tab', { hasText: name })

test.describe('guidance: what to fill in, objectives, capacity', () => {
  test.beforeEach(async ({ page }) => {
    await login(page)
  })

  test('a "no data" finding names the field to fill in and the button takes you to it', async ({ page }) => {
    await createProject(page, uniqueName('needs'), TEMPLATE)
    await tab(page, 'Требования').click()
    await page.locator('.reqs [data-req="rtoSeconds"] input').fill('')
    await tab(page, 'Проверки').click()

    const need = page.locator('.finding .need', { hasText: 'RTO' }).first()
    await expect(need).toBeVisible({ timeout: 15_000 })
    await need.click()
    await expect(tab(page, 'Требования')).toHaveClass(/active/)
    await expect(page.locator('.reqs [data-req="rtoSeconds"] input').first()).toBeFocused()

    // filling the value turns the question into an answer
    await page.locator('.reqs [data-req="rtoSeconds"] input').first().fill('30')
    await tab(page, 'Проверки').click()
    await expect(page.locator('.finding .need', { hasText: 'RTO' })).toHaveCount(0, { timeout: 15_000 })
  })

  test('a "no data" finding about an element opens its inspector at the right field', async ({ page }) => {
    await createProject(page, uniqueName('needs-prop'), 'Multi-region, pilot light')
    const id = page.url().split('/p/')[1]
    // remove the replication lag from every async connection: the engine can no longer verify the RPO
    await page.evaluate(async (pid) => {
      // the editor may be saving its own layout at that very moment: read, change and write again until the revision still matches
      for (let attempt = 0; attempt < 8; attempt++) {
        const p = await (await fetch('/api/projects/' + pid)).json()
        const arch = p.architecture
        for (const e of arch.edges) if (e.type === 'async') delete e.props.lagSeconds
        const r = await fetch('/api/projects/' + pid + '/draft', { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ architecture: arch, expectedRev: p.rev }) })
        if (r.ok) return
        if (r.status !== 409) throw new Error(await r.text())
        await new Promise((resolve) => setTimeout(resolve, 300))
      }
      throw new Error('the draft kept changing under the test')
    }, id)
    await page.reload()
    const need = page.locator('.finding .need', { hasText: 'Задержка репликации' }).first()
    await expect(need).toBeVisible({ timeout: 15_000 })
    await need.click()
    await expect(page.locator('.inspector')).toBeVisible()
    await expect(page.locator('.inspector [data-prop="lagSeconds"] input')).toBeFocused()
  })

  test('an objective pill replays the scenario behind its value', async ({ page }) => {
    await createProject(page, uniqueName('pills'), TEMPLATE)
    await expect(page.locator('button.obj').first()).toBeVisible({ timeout: 15_000 })
    await expect(page.locator('.whatif .res')).toHaveCount(0)
    await page.locator('button.obj').first().click() // RTO: the site failure that takes longest to recover from
    await expect(page.locator('.whatif .res')).toBeVisible()
  })

  test('the capacity plan answers "what if the load grows"', async ({ page }) => {
    await createProject(page, uniqueName('capacity'), TEMPLATE)
    await openAnalysis(page, 'Ёмкость')
    await expect(page.locator('.cap .svc').first()).toBeVisible({ timeout: 15_000 })
    const services = await page.locator('.cap .svc').count()
    expect(services).toBeGreaterThan(2)
    await expect(page.locator('.cap .tag-crit')).toHaveCount(0) // the template carries its declared peak

    await page.locator('.cap .slider').fill('4')
    await expect(page.locator('.cap .whatif .val')).toContainText('×4')
    await expect(page.locator('.cap .svc.crit').first()).toBeVisible()
    await expect(page.locator('.cap .tiles .tile.crit').first()).toBeVisible()

    await page.locator('.cap .presets .chip', { hasText: '×1' }).first().click()
    await expect(page.locator('.cap .svc.crit')).toHaveCount(0)
  })
})

test.describe('navigation: command palette and shortcuts', () => {
  test.beforeEach(async ({ page }) => {
    await login(page)
    await createProject(page, uniqueName('palette'), TEMPLATE)
    await expect(page.locator('.arch-node').first()).toBeVisible()
  })

  test('Ctrl+K finds a component, adds one and runs a failure scenario', async ({ page }) => {
    await page.keyboard.press('Control+k')
    await expect(page.locator('.palette')).toBeVisible()
    await page.keyboard.type('postgres primary')
    await expect(page.locator('.palette .item.on')).toContainText('PostgreSQL primary')
    await page.keyboard.press('Enter')
    await expect(page.locator('.palette')).toHaveCount(0)
    await expect(page.locator('.inspector header b')).toContainText('PostgreSQL primary')

    const before = await page.locator('.arch-node').count()
    await page.keyboard.press('Escape')
    await page.keyboard.press('Control+k')
    await page.keyboard.type('Добавить: Redis')
    await page.keyboard.press('Enter')
    await expect(page.locator('.arch-node')).toHaveCount(before + 1)

    await page.keyboard.press('Escape')
    await page.keyboard.press('Control+k')
    await page.keyboard.type('Симуляция: ЦОД-1')
    await expect(page.locator('.palette .item.on')).toContainText('ЦОД-1')
    await page.keyboard.press('Enter')
    await expect(page.locator('.whatif .res')).toBeVisible()
  })

  test('the palette lists what to do for an empty query, and Esc closes it', async ({ page }) => {
    await page.getByRole('button', { name: 'Поиск и команды' }).click()
    await expect(page.locator('.palette .item').first()).toBeVisible()
    await expect(page.locator('.palette .gh').first()).toBeVisible()
    await page.keyboard.type('qqqqqqzz')
    await expect(page.locator('.palette .none')).toBeVisible()
    await page.keyboard.press('Escape')
    await expect(page.locator('.palette')).toHaveCount(0)
  })

  test('"?" shows the keyboard shortcuts', async ({ page }) => {
    await page.locator('.canvas').click({ position: { x: 5, y: 400 } })
    await page.keyboard.press('Shift+?')
    await expect(page.locator('.modal', { hasText: 'Горячие клавиши' })).toBeVisible()
    await expect(page.locator('.modal kbd', { hasText: 'K' }).first()).toBeVisible()
    await page.keyboard.press('Escape')
    await expect(page.locator('.modal')).toHaveCount(0)
  })
})

test.describe('several nodes at once', () => {
  test.beforeEach(async ({ page }) => {
    await login(page)
    await createProject(page, uniqueName('multi'), TEMPLATE)
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await page.locator('.canvas').click({ position: { x: 5, y: 400 } })
  })

  test('select all, duplicate, undo, clear', async ({ page }) => {
    const n = await page.locator('.arch-node').count()
    await page.keyboard.press('Control+a')
    await expect(page.locator('.bulk header b')).toContainText(String(n))
    await page.locator('.bulk footer .btn', { hasText: 'Дублировать' }).click()
    await expect(page.locator('.arch-node')).toHaveCount(2 * n)
    await page.keyboard.press('Control+z')
    await expect(page.locator('.arch-node')).toHaveCount(n)
    await page.keyboard.press('Escape')
    await expect(page.locator('.bulk')).toHaveCount(0)
  })

  test('Shift+click extends the selection; they are grouped, moved to another zone and deleted together', async ({ page }) => {
    // both cards sit in the middle site, away from the inspector card that opens on the left
    const pg = page.locator('.arch-node', { hasText: 'PostgreSQL replica' })
    const redis = page.locator('.arch-node', { hasText: 'Redis (replica)' })
    // connection labels float over cards: click the top-left corner, which they do not cover
    const corner = { position: { x: 6, y: 6 } }
    const both = async () => {
      await page.locator('.canvas').click({ position: { x: 5, y: 400 } })
      await pg.click(corner)
      await page.keyboard.down('Shift')
      await redis.click(corner)
      await page.keyboard.up('Shift')
      await expect(page.locator('.bulk header b')).toContainText('2')
    }
    const n = await page.locator('.arch-node').count()

    await both()
    await expect(page.locator('.inspector')).toHaveCount(0)
    await page.getByRole('button', { name: 'Объединить в сервис…' }).click()
    await page.fill('.modal input', 'e2e-service')
    await page.keyboard.press('Enter')
    await expect(page.locator('.modal')).toHaveCount(0)
    await page.locator('.canvas').click({ position: { x: 5, y: 400 } })
    await redis.click(corner)
    await expect(page.locator('.inspector input[list="ia-groups"]')).toHaveValue('e2e-service')

    // move both to the third site: they are placed inside its rectangle
    await both()
    await page.locator('.bulk select').selectOption({ label: 'Площадка 3 · облако' })
    await page.locator('.canvas').click({ position: { x: 5, y: 400 } })
    await pg.click(corner)
    const zoneName = await page.locator('.inspector label.prop', { hasText: 'Площадка' }).locator('select').evaluate((el: HTMLSelectElement) => el.options[el.selectedIndex].text)
    expect(zoneName).toBe('Площадка 3 · облако')
    const inside = await page.evaluate(() => {
      const rect = (e: Element) => e.getBoundingClientRect()
      const zone = [...document.querySelectorAll('.arch-zone')].find((z) => z.textContent?.includes('Площадка 3'))!
      const zr = rect(zone)
      return [...document.querySelectorAll('.arch-node')].filter((c) => /PostgreSQL replica|Redis \(replica\)/.test(c.textContent ?? '')).every((c) => {
        const r = rect(c)
        return r.left >= zr.left - 1 && r.right <= zr.right + 1 && r.top >= zr.top - 1 && r.bottom <= zr.bottom + 1
      })
    })
    expect(inside).toBe(true)

    // delete both together, then undo both with one step
    await page.locator('.canvas').click({ position: { x: 5, y: 400 } })
    await both()
    await page.keyboard.press('Delete')
    await expect(page.locator('.arch-node')).toHaveCount(n - 2)
    await page.keyboard.press('Control+z')
    await expect(page.locator('.arch-node')).toHaveCount(n)
  })

  test('a rectangle (Shift+drag) selects the cards inside it', async ({ page }) => {
    const box = (await page.locator('.canvas').boundingBox())!
    await page.keyboard.down('Shift')
    await page.mouse.move(box.x + box.width * 0.3, box.y + 90)
    await page.mouse.down()
    await page.mouse.move(box.x + box.width * 0.95, box.y + box.height * 0.8, { steps: 12 })
    await page.mouse.up()
    await page.keyboard.up('Shift')
    await expect(page.locator('.bulk')).toBeVisible()
    const selected = await page.locator('.arch-node.multi').count()
    expect(selected).toBeGreaterThan(1)
    await expect(page.locator('.bulk header b')).toContainText(String(selected))
  })
})

test.describe('zooming out keeps the diagram readable', () => {
  test('cards switch to a compact form with a legible name, then to coloured tiles', async ({ page }) => {
    await login(page)
    await createProject(page, uniqueName('lod'), TEMPLATE)
    await expect(page.locator('.arch-node').first()).toBeVisible()
    const lod = () => page.locator('.canvas').getAttribute('data-lod')
    await page.locator('.zoom button[aria-label="Приблизить"]').click()
    await page.locator('.zoom button[aria-label="Приблизить"]').click()
    await page.locator('.zoom button[aria-label="Приблизить"]').click()
    await expect.poll(lod).toBe('full')
    await expect(page.locator('.arch-node .sub').first()).toBeVisible()
    for (let i = 0; i < 5; i++) await page.locator('.zoom button[aria-label="Отдалить"]').click()
    await expect.poll(lod).not.toBe('full')
    await expect(page.locator('.arch-node .sub').first()).toBeHidden()
    for (let i = 0; i < 4; i++) await page.locator('.zoom button[aria-label="Отдалить"]').click()
    await expect.poll(lod).toBe('mini')
    await expect(page.locator('.arch-node .txt').first()).toBeHidden()
  })
})

test.describe('projects', () => {
  test('cards show the diagram; a project can be duplicated; sorting follows the name', async ({ page }) => {
    await login(page)
    const a = uniqueName('aaa-thumb')
    const z = uniqueName('zzz-thumb')
    await createProject(page, z, TEMPLATE)
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await expect(page.locator('.crumbs .meta')).toContainText('сохранено', { timeout: 10_000 }) // the layout is stored
    await page.goto('/')
    await createProject(page, a, TEMPLATE)
    await page.goto('/')

    const card = page.locator('article.project', { hasText: z })
    await expect(card.locator('.thumb svg rect').first()).toBeVisible()
    expect(await card.locator('.thumb svg rect').count()).toBeGreaterThan(10)

    // duplicate
    await card.getByRole('button', { name: 'Дублировать' }).click()
    await page.fill('.modal input', `${z} копия`)
    await page.locator('.modal-foot .btn-primary').click()
    await expect(page.locator('article.project', { hasText: `${z} копия` })).toBeVisible({ timeout: 15_000 })
    await expect(page.locator('article.project', { hasText: `${z} копия` }).locator('.thumb svg rect').first()).toBeVisible()

    // sort by name: aaa… before zzz…
    await page.locator('select[aria-label="Сортировка"]').selectOption('name')
    // textContent: cards below the fold skip their rendering (content-visibility) and have no innerText
    const names = await page.locator('article.project h2').evaluateAll((els) => els.map((e) => (e.textContent ?? '').trim()))
    expect(names.indexOf(a)).toBeGreaterThanOrEqual(0)
    expect(names.indexOf(a)).toBeLessThan(names.indexOf(z))
    await page.reload()
    await expect(page.locator('select[aria-label="Сортировка"]')).toHaveValue('name') // remembered
  })

  test('thumbnails the list did not bring are loaded when the card scrolls into view', async ({ page }) => {
    await login(page)
    const name = uniqueName('lazy-thumb')
    await createProject(page, name, TEMPLATE)
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await expect(page.locator('.crumbs .meta')).toContainText('сохранено', { timeout: 10_000 })
    const setting = (value: number) =>
      page.evaluate(async (v) => {
        const r = await fetch('/api/admin/settings', { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ changes: { 'limits.listThumbnails': v } }) })
        if (!r.ok) throw new Error(await r.text())
      }, value)
    await setting(0) // no thumbnail comes with the list
    try {
      await page.goto('/')
      const card = page.locator('article.project', { hasText: name })
      await expect(card.locator('.thumb svg rect').first()).toBeVisible({ timeout: 15_000 })
      expect(await card.locator('.thumb svg rect').count()).toBeGreaterThan(10)
    } finally {
      await setting(60)
    }
  })
})

