import { expect, type Locator, type Page } from '@playwright/test'
import { test } from './fixtures'
import { createProject, login, openAnalysis, uniqueName } from './helpers'

/**
 * Every list of tabs follows the tab pattern (WAI-ARIA "Tabs"): one stop of the Tab key, the arrows / Home / End move the focus AND the selection, and the selected tab
 * and the panel it shows name each other. The lists are judged on the tabs they have today, read from the page: other panels add views, a failure on screen adds a tab,
 * and a test that names them would break the day somebody adds one.
 */
test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => { try { localStorage.setItem('ia_motion', 'off') } catch { /* no storage: the default motion applies */ } })
  await login(page)
})

// the tabs of THIS list: the analysis panel, drawn under the tab of the right panel, has a list of its own, and a locator that reached into it would count its tabs too
const tabsOf = (list: Locator) => list.locator(':scope > [role="tab"]')

/** What each tab says about itself, in the order of the page. */
const state = (list: Locator) => tabsOf(list).evaluateAll((els) => els.map((e) => ({
  id: e.id, tab: (e as HTMLElement).dataset.tab ?? '', selected: e.getAttribute('aria-selected') === 'true', tabindex: e.getAttribute('tabindex'), controls: e.getAttribute('aria-controls'), focused: document.activeElement === e,
})))

async function selectedIndex(list: Locator) {
  return (await state(list)).findIndex((t) => t.selected)
}

/** The pattern, on whatever tabs the list has. `settle` waits for a panel that is drawn after an animation. */
async function followsPattern(page: Page, list: Locator, what: string) {
  await expect(list, `${what}: it is a list of tabs`).toHaveAttribute('role', 'tablist')
  expect((await list.getAttribute('aria-label')) ?? '', `${what}: the list has a name`).not.toBe('')
  const n = await tabsOf(list).count()
  expect(n, `${what}: more than one tab`).toBeGreaterThan(1)

  // one stop of the Tab key, and it is the selected tab
  const at = await state(list)
  expect(at.filter((t) => t.selected), `${what}: one tab is selected`).toHaveLength(1)
  expect(at.filter((t) => t.tabindex === '0').map((t) => t.id), `${what}: only the selected tab has tabindex 0`).toEqual(at.filter((t) => t.selected).map((t) => t.id))
  expect(at.filter((t) => t.tabindex === '-1'), `${what}: the others are -1`).toHaveLength(n - 1)
  expect(new Set(at.map((t) => t.id)).size, `${what}: every tab has an id of its own`).toBe(n)

  // the arrows, Home and End move the focus and the selection together, and wrap
  const walk = async (key: string, expected: (from: number) => number) => {
    const from = await selectedIndex(list)
    await page.keyboard.press(key)
    await expect.poll(async () => {
      const now = await state(list)
      const sel = now.findIndex((t) => t.selected)
      return { sel, focused: now.findIndex((t) => t.focused), stops: now.filter((t) => t.tabindex === '0').length }
    }, { message: `${what}: ${key} from tab ${from}` }).toEqual({ sel: expected(from), focused: expected(from), stops: 1 })
  }
  await tabsOf(list).nth(await selectedIndex(list)).focus()
  for (let i = 0; i < n; i++) await walk('ArrowRight', (from) => (from + 1) % n) // all the way round
  await walk('End', () => n - 1)
  await walk('ArrowRight', () => 0) // wraps to the first
  await walk('ArrowLeft', () => n - 1) // and back
  await walk('Home', () => 0)

  // the selected tab and its panel name each other
  const sel = tabsOf(list).nth(await selectedIndex(list))
  const controls = await sel.getAttribute('aria-controls')
  expect(controls, `${what}: the selected tab says which panel it controls`).toBeTruthy()
  const panel = page.locator(`[id="${controls}"]`)
  await expect(panel, `${what}: that panel is there`).toHaveCount(1)
  await expect(panel).toHaveAttribute('role', 'tabpanel')
  await expect(panel).toHaveAttribute('aria-labelledby', (await sel.getAttribute('id'))!)
  // and the tabs that are not selected promise nothing (their panels are not drawn)
  for (const t of (await state(list)).filter((x) => !x.selected)) expect(t.controls, `${what}: ${t.tab}`).toBeNull()
}

test.describe('tab lists', () => {
  test('the right panel', async ({ page }) => {
    await createProject(page, uniqueName('tabs-right'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await followsPattern(page, page.locator('.right .head [role="tablist"]'), 'right panel')
  })

  test('the analysis views', async ({ page }) => {
    await createProject(page, uniqueName('tabs-analysis'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await openAnalysis(page, 'Ёмкость')
    await followsPattern(page, page.locator('.analysis [role="tablist"]'), 'analysis')
  })

  test('the export dialog', async ({ page }) => {
    await createProject(page, uniqueName('tabs-export'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur())
    await page.keyboard.press('e')
    const list = page.getByRole('dialog').locator('[role="tablist"]')
    await expect(tabsOf(list).first()).toBeVisible({ timeout: 20_000 })
    await followsPattern(page, list, 'export')
  })

  test('the editor of a prompt in the admin panel', async ({ page }) => {
    await page.goto('/admin/prompts')
    await page.locator('#adm-main .idcell .linklike').first().click()
    const list = page.getByRole('dialog').locator('[role="tablist"]')
    await expect(tabsOf(list).first()).toBeVisible()
    await followsPattern(page, list, 'prompt editor')
    // the panel is the one wrapper around both forms of the prompt
    await expect(page.getByRole('dialog').locator('[role="tabpanel"]')).toHaveCount(1)
  })

  test('the lists on one page do not share an id, with the right panel, the analysis and the export dialog drawn together', async ({ page }) => {
    await createProject(page, uniqueName('tabs-ids'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await openAnalysis(page, 'Ёмкость')
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur())
    await page.keyboard.press('e')
    await expect(tabsOf(page.getByRole('dialog').locator('[role="tablist"]')).first()).toBeVisible({ timeout: 20_000 })
    const ids = await page.evaluate(() => Array.from(document.querySelectorAll('[role="tab"], [role="tabpanel"]')).map((e) => e.id))
    expect(ids.length).toBeGreaterThanOrEqual(8)
    expect(ids.every(Boolean)).toBe(true)
    expect(new Set(ids).size).toBe(ids.length)
  })
})

test.describe('tabs and a failure on screen', () => {
  test.beforeEach(async ({ page }) => {
    expect((await page.request.put('/api/admin/settings', { data: { changes: { 'editor.failureLeadMs': 0 } } })).ok()).toBeTruthy()
  })
  test.afterEach(async ({ page }) => {
    await page.request.put('/api/admin/settings', { data: { changes: { 'editor.failureLeadMs': 2400 } } })
  })

  test('the failure has a tab; the arrows on it move between tabs and do not step the playback; when it goes the focus goes to the tab that is shown', async ({ page }) => {
    await createProject(page, uniqueName('tabs-sim'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    const list = page.locator('.right .head [role="tablist"]')
    const before = await tabsOf(list).count()
    await page.locator('.whatif button', { hasText: 'ЦОД-1' }).click()
    await expect(page.getByTestId('simcard')).toBeVisible({ timeout: 15_000 })
    await expect(tabsOf(list)).toHaveCount(before + 1)
    await followsPattern(page, list, 'right panel with a failure')

    // the story of the failure is told over the diagram; its words are the step it is at (the figure beside them moves by itself, so it is not compared)
    const caption = page.getByTestId('story-caption')
    const words = caption.locator('.txt')
    const story = await words.innerText()
    const sim = list.locator('[role="tab"][data-tab="sim"]')
    await sim.focus()
    await page.keyboard.press('ArrowRight')
    await page.keyboard.press('ArrowRight')
    await page.keyboard.press('ArrowLeft')
    await expect(words).toHaveText(story)
    await sim.focus()

    // Escape closes the failure: its tab goes while it has the focus, and the focus is on the tab that is shown, not on the page
    await page.keyboard.press('Escape')
    await expect(tabsOf(list)).toHaveCount(before)
    await expect.poll(() => page.evaluate(() => document.activeElement?.getAttribute('role'))).toBe('tab')
    await expect(list.locator('[role="tab"]:focus')).toHaveAttribute('aria-selected', 'true')
  })
})
