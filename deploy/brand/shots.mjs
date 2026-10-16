// The screenshots of the README (docs/images/*.png), taken from a running stack with the demo project `shop-prod` in it.
//
//   cd frontend && E2E_EMAIL=<admin> E2E_PASSWORD=<password> node ../deploy/brand/shots.mjs [name ...]
//
// Names: editor sim availability capacity library product placement gallery export admin social ai. Without names — all of them but `ai`: that one is a real answer of the model,
// so it needs an OpenRouter key (Administration → OpenRouter), takes a couple of minutes and is taken only when asked for by name. The AI should be switched on in the stack for all of them:
// with no key the prompt bar of the editor says that the AI is unavailable, and that is not what a picture of the product should say. The interface is shown in Russian.
// BASE_URL (default http://localhost:8080) points at the stack, PW_CHROMIUM at a browser to use instead of the one Playwright installed, SHOTS_OUT at the folder.
import { createRequire } from 'node:module'
import { readFileSync } from 'node:fs'
import path from 'node:path'
import { pathToFileURL } from 'node:url'

const { chromium } = createRequire(pathToFileURL(path.join(process.cwd(), 'package.json')))('@playwright/test')
const base = process.env.BASE_URL ?? 'http://localhost:8080'
const out = path.resolve(process.env.SHOTS_OUT ?? path.join(process.cwd(), '../docs/images'))
const email = process.env.E2E_EMAIL
const password = process.env.E2E_PASSWORD
if (!email || !password) throw new Error('set E2E_EMAIL and E2E_PASSWORD (an administrator of the stack)')
const names = process.argv.slice(2)
const want = (n) => (names.length ? names.includes(n) : n !== 'ai')

const browser = await chromium.launch({ executablePath: process.env.PW_CHROMIUM || undefined, args: ['--no-sandbox'] })
const ctx = await browser.newContext({ viewport: { width: 1600, height: 984 }, deviceScaleFactor: 1.25, locale: 'ru-RU', baseURL: base })
await ctx.addInitScript(() => { try { localStorage.setItem('ia_tour_off', '1') } catch { /* no storage: the tour may open */ } })
const page = await ctx.newPage()

const put = (changes) => page.request.put('/api/admin/settings', { data: { changes } })
/** A picture of the page, or of one element of it. */
async function shot(name, { wait = 1200, of } = {}) {
  await page.waitForTimeout(wait)
  // the failure panel keeps the current step in view; for a picture it starts from the top, where the figures are
  await page.locator('.right .simcard .scroll').evaluate((e) => { e.scrollTop = 0 }).catch(() => undefined)
  await page.waitForTimeout(250)
  await (of ?? page).screenshot({ path: path.join(out, `${name}.png`) })
  console.log('shot', name)
}
async function openShop() {
  await page.goto('/')
  await page.locator('.project', { hasText: 'shop-prod' }).first().click()
  await page.waitForSelector('.arch-node')
  await page.waitForTimeout(1500)
}
const analysis = async (segment) => {
  await page.locator('.right .tab', { hasText: 'Анализ' }).click()
  await page.locator('.right .seg .segbtn', { hasText: segment }).click()
}

await page.goto('/login')
await page.fill('#email', email)
await page.fill('#password', password)
await page.click('button.btn-primary')
await page.waitForURL(/\/$/)
const defaults = (await (await page.request.get('/api/admin/settings')).json()).settings
let leadChanged = false

try {
  if (want('editor')) { await openShop(); await shot('editor', { wait: 2200 }) }

  if (want('sim')) {
    await put({ 'editor.failureLeadMs': 0 })   // a failure starts at once: no lead-in to wait for
    leadChanged = true
    await openShop()
    await page.locator('.whatif button', { hasText: 'ЦОД-1' }).first().click()
    await page.waitForSelector('[data-testid="simcard"]')
    await page.waitForTimeout(800)
    const steps = page.locator('.steps > li')
    const n = await steps.count()
    await steps.nth(0).click(); await shot('traffic-before', { wait: 2200 })
    await steps.nth(1).click(); await shot('simulation', { wait: 2200 })
    await steps.nth(n - 1).click(); await shot('traffic-settled', { wait: 2200 })
    await shot('sim-panel', { wait: 600 })
    // the panel in a window, moved over the design
    await page.getByTestId('sim-detach').click()
    await page.waitForSelector('[data-testid="float-panel"]')
    const box = await page.getByTestId('float-panel').boundingBox()
    await page.waitForTimeout(500)
    await page.mouse.move(box.x + 140, box.y + 20); await page.mouse.down(); await page.mouse.move(box.x - 520, box.y + 20, { steps: 10 }); await page.mouse.up()
    await page.locator('[data-testid="float-panel"] .scroll').evaluate((e) => { e.scrollTop = 0 }).catch(() => undefined)
    await shot('sim-window')
    await page.getByTestId('float-dock').click()
    // several failures at once
    await page.getByTestId('whatif-compose').click()
    await page.locator('.compose .items').evaluate((e) => { e.scrollTop = 0 })
    await shot('compose', { wait: 800 })
  }

  if (want('availability')) { await openShop(); await analysis('Доступность'); await shot('availability', { wait: 1600, of: page.getByTestId('right-panel') }) }
  if (want('capacity')) { await openShop(); await analysis('Ёмкость'); await shot('capacity', { wait: 1400, of: page.getByTestId('right-panel') }) }

  if (want('library')) {
    await openShop()
    // the categories fold; the counts say how many products there are, "More N" holds the rest of a job
    for (const id of ['templates', 'compute', 'security', 'apps', 'ai']) await page.getByTestId('lib-cat-' + id).click()
    await page.locator('.library .scroll').evaluate((e) => { e.scrollTop = 0 })
    await shot('library', { wait: 900, of: page.locator('.library') })
  }

  if (want('product')) {
    await openShop()
    await page.locator('.arch-node', { hasText: 'PostgreSQL' }).first().click()
    await shot('product', { of: page.locator('.inspector') })
  }

  if (want('placement')) {
    await page.goto('/')
    await page.getByRole('button', { name: 'Новый проект' }).first().click()
    await page.locator('.wizard-card').click()
    await page.waitForSelector('.wiz .site')
    await page.locator('.wiz .quick .chip', { hasText: /^5$/ }).click()
    await page.locator('.wiz .cfg').evaluate((el) => { el.scrollTop = 0 })
    await shot('placement', { wait: 2500 })
  }

  if (want('gallery')) {
    await page.goto('/')
    await page.getByRole('button', { name: 'Новый проект' }).first().click()
    await page.waitForSelector('[data-testid="gallery"]')
    await page.locator('.modal input.input').first().fill('Платёжный шлюз')
    await page.locator('[data-testid="gallery"] [data-tag="cloud"]').click()
    await shot('gallery', { wait: 900 })
  }

  if (want('export')) {
    await openShop()
    await page.getByRole('button', { name: 'Экспорт' }).first().click()
    await page.locator('.formats button', { hasText: 'Манифесты Kubernetes' }).click()
    await page.waitForSelector('[data-test="export-summary"]', { timeout: 30_000 })
    await shot('export')
  }

  if (want('admin')) {
    await page.goto('/admin/rules')
    await page.waitForSelector('#adm-main')
    await shot('admin')
  }

  if (want('social')) {
    // the card of the repository when its link is shared (GitHub → Settings → Social preview takes 1280×640): the banner on the ground of the product
    const card = await ctx.newPage()
    await card.setViewportSize({ width: 1280, height: 640 })
    const svg = readFileSync(path.join(out, 'banner.ru.svg'), 'utf8')
    await card.setContent(`<body style="margin:0;height:640px;display:grid;place-items:center;background:#14171f;overflow:hidden"><div style="width:1180px">${svg.replace(/width="1000" height="360"/, 'width="1180" height="425"')}</div></body>`)
    await card.waitForTimeout(500)
    await card.screenshot({ path: path.join(out, 'social.png') })
    await card.close()
    console.log('shot social')
  }

  if (names.includes('ai')) {
    // a new project, a request in words, the model's proposal with its diff and its score — before anything is applied
    const made = await page.request.post('/api/projects', { data: { name: 'AI' } })
    const id = (await made.json()).id
    try {
      await page.goto(`/p/${id}`)
      await page.locator('.promptbar .pin').fill('Веб-сервис в AWS в двух зонах доступности одного региона: приложение в Amazon EKS, управляемая база Amazon RDS Multi-AZ PostgreSQL и Amazon ElastiCache для Redis. 500 rps, RTO 15 минут, RPO 5 минут. Резервный регион не нужен.')
      await page.locator('.promptbar .go').click()
      await page.waitForSelector('.proposal', { timeout: 300_000 })
      await shot('ai', { wait: 2000 })
    } finally {
      await page.request.delete(`/api/projects/${id}`).catch(() => undefined)
    }
  }
} finally {
  if (leadChanged) await put({ 'editor.failureLeadMs': defaults.find((x) => x.key === 'editor.failureLeadMs')?.default })
  await browser.close()
}
