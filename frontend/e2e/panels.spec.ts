import { expect, type Locator, type Page } from '@playwright/test'
import { test } from './fixtures'
import { createProject, login, uniqueName } from './helpers'

// The panels of the editor are the user's to arrange: the right panel folds to a rail and is dragged to the width wanted, the what-if bar and the
// failure panel are moved over the diagram and folded, and the editor remembers where everything was left (per browser).

const box = async (l: Locator) => (await l.boundingBox())!
/** The box once it has stopped changing: a window pops in with a short animation. */
async function settled(l: Locator) {
  let last = await box(l)
  for (let i = 0; i < 25; i++) {
    await l.page().waitForTimeout(80)
    const now = await box(l)
    if (now.x === last.x && now.y === last.y && now.width === last.width && now.height === last.height) return now
    last = now
  }
  return last
}

/** Drags with real pointer events (the panels listen to the pointer, not to the HTML drag and drop). */
async function drag(page: Page, from: { x: number; y: number }, by: { dx: number; dy: number }) {
  await page.mouse.move(from.x, from.y)
  await page.mouse.down()
  await page.mouse.move(from.x + by.dx / 2, from.y + by.dy / 2, { steps: 4 })
  await page.mouse.move(from.x + by.dx, from.y + by.dy, { steps: 4 })
  await page.mouse.up()
}
const centre = (b: { x: number; y: number; width: number; height: number }) => ({ x: b.x + b.width / 2, y: b.y + b.height / 2 })

test.describe('panels', () => {
  test.beforeEach(async ({ page }) => {
    await login(page)
    // a failure that has just been run plays itself; these tests look at the panels, not at the story
    expect((await page.request.put('/api/admin/settings', { data: { changes: { 'editor.failureLeadMs': 0 } } })).ok()).toBeTruthy()
    await createProject(page, uniqueName('panels'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
  })
  test.afterEach(async ({ page }) => {
    await page.request.put('/api/admin/settings', { data: { changes: { 'editor.failureLeadMs': 2400 } } })
  })

  test('the right panel folds to a rail of its tabs, opens on the tab picked, and the key ] does the same', async ({ page }) => {
    const right = page.locator('[data-testid="right-panel"]')
    const open = (await box(right)).width
    expect(open).toBeGreaterThan(300)

    await page.locator('[data-testid="right-fold"]').click()
    await expect(page.locator('[data-testid="right-unfold"]')).toBeVisible()
    await expect.poll(async () => (await box(right)).width).toBeLessThan(80)
    // the rail has a button for every tab, and a tab picked there opens the panel on it
    await right.getByRole('button', { name: 'Анализ' }).click()
    await expect(page.locator('[data-testid="right-fold"]')).toBeVisible()
    await expect(page.locator('.right .tab.active')).toHaveText(/Анализ/)
    await expect.poll(async () => (await box(right)).width).toBeGreaterThan(300)

    // by the position of the key: the Russian layout has it too
    await page.locator('.canvas').click({ position: { x: 5, y: 400 } })
    await page.keyboard.press('BracketRight')
    await expect(page.locator('[data-testid="right-unfold"]')).toBeVisible()
    await page.keyboard.press('BracketRight')
    await expect(page.locator('[data-testid="right-fold"]')).toBeVisible()
  })

  test('the width is dragged by the edge, kept after a reload, stepped with the arrows and brought back by a double click', async ({ page }) => {
    const right = page.locator('[data-testid="right-panel"]')
    const edge = page.locator('[data-testid="right-edge"]')
    const usual = (await box(right)).width

    const e = await settled(edge)
    await drag(page, { x: e.x + e.width / 2, y: e.y + 300 }, { dx: -160, dy: 0 })
    const wider = (await settled(right)).width
    expect(wider).toBeGreaterThan(usual + 100)

    await page.reload()
    await expect(page.locator('.arch-node').first()).toBeVisible()
    expect(Math.abs((await box(right)).width - wider)).toBeLessThan(2)

    // the arrows step the edge: left widens, right narrows
    await edge.focus()
    await page.keyboard.press('ArrowLeft')
    await expect.poll(async () => (await box(right)).width).toBeGreaterThan(wider + 10)
    await page.keyboard.press('ArrowRight')
    await page.keyboard.press('ArrowRight')
    await expect.poll(async () => (await box(right)).width).toBeLessThan(wider)

    // it is never narrower than the administrator allows (editor.rightPanelMinWidth), nor wider than the limit. The panel's width is animated:
    // the edge is where it ends up, not where the last arrow key found it (a drag started on the old place misses the edge)
    const e2 = await settled(edge)
    await drag(page, { x: e2.x + e2.width / 2, y: e2.y + 300 }, { dx: 900, dy: 0 })
    expect(Math.round((await settled(right)).width)).toBe(Number(await edge.getAttribute('aria-valuemin')))
    const e3 = await settled(edge)
    await drag(page, { x: e3.x + e3.width / 2, y: e3.y + 300 }, { dx: -1500, dy: 0 })
    expect((await settled(right)).width).toBeLessThanOrEqual(Number(await edge.getAttribute('aria-valuemax')))

    await edge.dblclick()
    await expect.poll(async () => Math.round((await box(right)).width)).toBe(Math.round(usual))
  })

  test('the what-if bar is dragged by what is not a button, stays where it is put, folds to a pill and goes back', async ({ page }) => {
    const bar = page.locator('[data-testid="whatif"]')
    const docked = await box(bar)
    expect(await bar.getAttribute('data-overlay')).toBe('bottom')

    // by its label: not a control
    const label = await box(bar.locator('.lbl'))
    await drag(page, centre(label), { dx: 150, dy: -260 })
    const moved = await box(bar)
    expect(moved.y).toBeLessThan(docked.y - 150)
    expect(moved.x).toBeGreaterThan(docked.x + 100)
    // it no longer reserves room at the bottom of the canvas
    expect(await bar.getAttribute('data-overlay')).toBe('float')

    // a click on a button is a click, not a drag
    await bar.getByRole('button', { name: 'ЦОД-1' }).click()
    await expect(page.locator('[data-testid="simcard"]')).toBeVisible({ timeout: 15_000 })
    expect(Math.abs((await box(bar)).x - moved.x)).toBeLessThan(2)

    await page.reload()
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await expect.poll(async () => Math.abs((await box(bar)).y - moved.y)).toBeLessThan(2)

    // folded it is a pill, and the failure on screen still shows its outcome in it
    await bar.getByRole('button', { name: 'ЦОД-1' }).click()
    await expect(page.locator('[data-testid="simcard"]')).toBeVisible({ timeout: 15_000 })
    await page.locator('[data-testid="whatif-fold"]').click()
    await expect(bar.getByRole('button', { name: 'ЦОД-2' })).toHaveCount(0)
    await expect(bar.locator('.what')).toContainText('ЦОД-1')
    await expect(bar.locator('.res')).toBeVisible()
    expect((await box(bar)).width).toBeLessThan(moved.width - 100)
    await page.locator('[data-testid="whatif-unfold"]').click()
    await expect(bar.getByRole('button', { name: 'ЦОД-2' })).toBeVisible()

    // Home on the grip (or a double click on it) puts the bar back where it belongs
    await page.locator('[data-testid="whatif-grip"]').dblclick()
    // (the docked bar sits on the bottom edge: what is compared is that edge, the bar itself is taller with an outcome in it)
    await expect.poll(async () => { const b = await box(bar); return Math.abs(b.y + b.height - (docked.y + docked.height)) }).toBeLessThan(2)
    expect(await bar.getAttribute('data-overlay')).toBe('bottom')
  })

  test('the failure panel leaves the right panel for a window over the diagram: moved, folded, put back, closed', async ({ page }) => {
    await page.locator('.whatif button', { hasText: 'ЦОД-1' }).click()
    const card = page.locator('[data-testid="simcard"]')
    await expect(card).toBeVisible({ timeout: 15_000 })
    await expect(page.locator('.right .tab.active')).toHaveText(/Отказ/)

    // detached: the tab is gone, the same card is a window
    await page.locator('[data-testid="sim-detach"]').click()
    const win = page.locator('[data-testid="float-panel"]')
    await expect(win).toBeVisible()
    await expect(page.locator('.right .tab', { hasText: 'Отказ' })).toHaveCount(0)
    await expect(win.locator('[data-testid="simcard"]')).toBeVisible()
    await expect(win.locator('[data-testid="float-served"]')).toContainText('%')
    // it does not move the diagram out of its way
    expect(await win.getAttribute('data-overlay')).toBe('float')

    // moved by the header
    const b0 = await settled(win)
    await drag(page, { x: b0.x + 130, y: b0.y + 20 }, { dx: -260, dy: 40 })
    const b1 = await box(win)
    expect(b1.x).toBeLessThan(b0.x - 200)
    // it stays inside the canvas wherever it is dragged
    const area = await box(page.locator('.center'))
    await drag(page, { x: b1.x + 130, y: b1.y + 20 }, { dx: -3000, dy: -3000 })
    const b2 = await box(win)
    expect(b2.x).toBeGreaterThanOrEqual(area.x)
    expect(b2.y).toBeGreaterThanOrEqual(area.y)

    // folded to its header, which still says what is happening
    await page.locator('[data-testid="float-fold"]').click()
    await expect(win.locator('[data-testid="simcard"]')).toBeHidden()
    expect((await box(win)).height).toBeLessThan(80)
    await expect(win.locator('[data-testid="float-served"]')).toBeVisible()

    // where it was left is where it is after a reload
    const left = await box(win)
    await page.reload()
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await page.locator('.whatif button', { hasText: 'ЦОД-1' }).click()
    await expect(win).toBeVisible({ timeout: 15_000 })
    await expect(win).toHaveClass(/folded/)
    await expect.poll(async () => Math.abs((await box(win)).x - left.x)).toBeLessThan(2)
    await page.locator('[data-testid="float-fold"]').click()
    await expect(win.locator('[data-testid="simcard"]')).toBeVisible()

    // put back into the right panel (opened if it was folded)
    await page.locator('[data-testid="right-fold"]').click()
    await page.locator('[data-testid="float-dock"]').click()
    await expect(win).toHaveCount(0)
    await expect(page.locator('[data-testid="right-fold"]')).toBeVisible()
    await expect(page.locator('.right .tab.active')).toHaveText(/Отказ/)

    // detached again, and closing the window ends the failure
    await page.locator('[data-testid="sim-detach"]').click()
    await page.locator('[data-testid="float-close"]').click()
    await expect(win).toHaveCount(0)
    await expect(page.locator('.arch-node:is(.sim-down, .sim-stalled, .sim-partial, .sim-promoted)')).toHaveCount(0)
  })

  test('the window and the panels can be driven from the keyboard', async ({ page }) => {
    await page.locator('.whatif button', { hasText: 'ЦОД-1' }).click()
    await expect(page.locator('[data-testid="simcard"]')).toBeVisible({ timeout: 15_000 })
    await page.locator('[data-testid="sim-detach"]').click()
    const win = page.locator('[data-testid="float-panel"]')
    await expect(win).toBeVisible()
    const b0 = await settled(win)
    await page.locator('[data-testid="float-grip"]').focus()
    await page.keyboard.press('ArrowLeft')
    await page.keyboard.press('ArrowLeft')
    await expect.poll(async () => (await box(win)).x).toBeLessThan(b0.x - 20)
    await page.keyboard.press('Home')
    await expect.poll(async () => Math.abs((await box(win)).x - b0.x)).toBeLessThan(2)
    // the story keeps its keys while the window is up
    await page.locator('.canvas').click({ position: { x: 5, y: 400 } })
    await page.keyboard.press('Escape')
    await expect(win).toHaveCount(0)
  })
})
