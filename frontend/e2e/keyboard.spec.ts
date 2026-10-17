import { expect, type Page } from '@playwright/test'
import { test } from './fixtures'
import { createProject, login, settled, uniqueName } from './helpers'

/**
 * The diagram by keyboard. The whole diagram is ONE stop of the Tab key (a card with tabindex 0, the others -1); the arrow keys walk from card to card, Enter does
 * what a click does, Shift+Enter extends the selection, Delete removes the card that has the focus. Vue Flow's own keyboard layer is off: it selected a card inside the
 * flow only, moved the selected card with the arrow keys without saving it, and spoke English.
 */
test.beforeEach(async ({ page }) => {
  // the view glides to a card in full motion; the tests look at where it ends
  await page.addInitScript(() => { try { localStorage.setItem('ia_motion', 'off') } catch { /* no storage: the default motion applies */ } })
  await login(page)
})

const card = (page: Page, text: string) => page.locator('.arch-node', { hasText: text }).first()
const tool = (page: Page, name: string) => page.locator('.toolbar .tool', { hasText: name })
const focusedCard = (page: Page) => page.locator('.canvas [data-kbd]:focus')
/** The project that is open, as the server has it. */
async function stored(page: Page) {
  const id = page.url().split('/p/')[1]
  return (await (await page.request.get(`/api/projects/${id}`)).json()) as { architecture: { nodes: { id: string; ui: { x: number; y: number } }[]; edges: unknown[] } }
}

test.describe('the diagram is one stop of the Tab key and nothing in it moves by itself', () => {
  test('one card has the stop, no wrapper and no link is focusable, Tab leaves the diagram in one press', async ({ page }) => {
    await createProject(page, uniqueName('kbd-stop'), '10 площадок')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await settled(page)
    const cardsAndHeads = await page.locator('.arch-node, .zone-head').count()
    expect(cardsAndHeads).toBeGreaterThan(50) // 62 stops on this design before: one per card and one per zone
    await expect(page.locator('.canvas [data-kbd][tabindex="0"]')).toHaveCount(1)
    await expect(page.locator('.canvas [data-kbd][tabindex="-1"]')).toHaveCount(cardsAndHeads - 1)
    await expect(page.locator('.vue-flow__node[tabindex]')).toHaveCount(0)
    await expect(page.locator('.vue-flow__edge[tabindex]')).toHaveCount(0)
    // Vue Flow's own description of a card (English) and its live region are gone
    await expect(page.locator('[id^="vue-flow__aria-live"]')).toHaveCount(0)
    await expect(page.locator('[aria-describedby^="vue-flow__node-desc"]')).toHaveCount(0)

    await page.locator('.canvas [data-kbd][tabindex="0"]').focus()
    await page.keyboard.press('Tab')
    expect(await page.evaluate(() => !!document.activeElement?.closest('.vue-flow') || !!document.activeElement?.closest('[data-kbd]'))).toBe(false)
    expect(await page.evaluate(() => !!document.activeElement?.closest('.canvas'))).toBe(true) // the toolbar of the canvas
    await page.keyboard.press('Shift+Tab')
    await expect(page.locator('.canvas [data-kbd]:focus')).toHaveCount(1)
  })

  test('the stop goes with the focus, and a card that goes does not leave the diagram without one', async ({ page }) => {
    await createProject(page, uniqueName('kbd-roving'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await expect(page.locator('.canvas [data-kbd][tabindex="0"]')).toHaveCount(1)
    const first = await page.locator('.canvas [data-kbd][tabindex="0"]').getAttribute('data-id')
    // the first card in reading order is the one at the top
    await expect(page.locator('.canvas .arch-node[tabindex="0"]')).toHaveAttribute('data-id', first!)
    const second = card(page, 'Kafka')
    await second.focus()
    await expect(second).toHaveAttribute('tabindex', '0')
    await expect(page.locator('.canvas [data-kbd][tabindex="0"]')).toHaveCount(1)
    // the card that has the stop is deleted: another one takes it over
    await page.keyboard.press('Delete')
    await expect(page.locator('.canvas [data-kbd][tabindex="0"]')).toHaveCount(1)
    await expect(page.locator('.arch-node:focus')).toHaveCount(1)
  })

  test('the arrow keys walk from card to card; they do not move a card, do not save anything and do not speak English', async ({ page }) => {
    await createProject(page, uniqueName('kbd-arrows'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await settled(page)
    await expect(page.locator('.crumbs .meta')).toContainText('сохранено', { timeout: 10_000 })
    const before = await stored(page)
    const moved = before.architecture.nodes[0].id
    const places = () => page.locator('.vue-flow__node-card').evaluateAll((els) => els.map((e) => [e.getAttribute('data-id'), (e as HTMLElement).style.transform]))
    const at = await places()

    await page.locator(`.arch-node[data-id="${moved}"]`).focus()
    await page.keyboard.press('Enter') // selected: this is where the arrow keys used to move it
    await expect(page.locator('.arch-node.selected')).toHaveCount(1)
    const seen = new Set<string>([moved])
    for (const key of ['ArrowRight', 'ArrowDown', 'ArrowRight', 'ArrowDown']) {
      await page.keyboard.press(key)
      seen.add((await focusedCard(page).getAttribute('data-id')) ?? '')
    }
    expect(seen.size).toBeGreaterThan(2) // the focus walked over other cards
    expect(await places()).toEqual(at) // no card is anywhere else than it was
    await expect(page.locator('.crumbs .meta')).toContainText('сохранено')
    await expect(page.locator('.zoom').getByRole('button', { name: 'Отменить' })).toBeDisabled()
    await page.reload()
    await expect(page.locator('.arch-node').first()).toBeVisible()
    const after = await stored(page)
    expect(after.architecture.nodes.map((n) => [n.id, n.ui.x, n.ui.y])).toEqual(before.architecture.nodes.map((n) => [n.id, n.ui.x, n.ui.y]))
  })
})

test.describe('Enter does what a click does', () => {
  test('Enter selects and opens the inspector; Shift+Enter extends; Enter on the selected card goes to its properties and Escape comes back', async ({ page }) => {
    await createProject(page, uniqueName('kbd-select'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    const a = card(page, 'Kafka')
    await a.focus()
    await page.keyboard.press('Enter')
    await expect(page.locator('.arch-node.selected')).toHaveCount(1)
    await expect(a).toHaveAttribute('aria-pressed', 'true')
    await expect(page.locator('.inspector')).toBeVisible()
    // the focus stays on the card (the inspector is not a trap)
    await expect(a).toBeFocused()

    // a second card joins with Shift+Enter: two are selected, the card with the actions on several appears
    const b = card(page, 'PostgreSQL primary')
    await b.focus()
    await page.keyboard.press('Shift+Enter')
    await expect(page.locator('.arch-node.multi')).toHaveCount(2)
    await expect(page.locator('.bulk')).toBeVisible()
    // and leaves with the same key
    await page.keyboard.press('Shift+Enter')
    await expect(page.locator('.arch-node.multi')).toHaveCount(0)
    await expect(page.locator('.arch-node.selected')).toHaveCount(1)

    // Enter on the card that is the selection: into its properties; Escape: back to the card, the selection stays
    await a.focus()
    await page.keyboard.press('Enter')
    await expect.poll(() => page.evaluate(() => !!document.activeElement?.closest('.inspector'))).toBe(true)
    await page.keyboard.press('Escape')
    await expect(a).toBeFocused()
    await expect(page.locator('.arch-node.selected')).toHaveCount(1)
    await expect(page.locator('.inspector')).toBeVisible()
    // Escape on the card clears the selection, and the keyboard stays where it was
    await page.keyboard.press('Escape')
    await expect(page.locator('.arch-node.selected')).toHaveCount(0)
    await expect(page.locator('.inspector')).toHaveCount(0)
    await expect(a).toBeFocused()
  })

  test('with the connect tool, Enter on the source and Enter on the target make exactly one link', async ({ page }) => {
    await createProject(page, uniqueName('kbd-connect'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    const edges = page.locator('.vue-flow__edge')
    const before = await edges.count()
    const a = card(page, 'GitOps')
    await a.focus()
    await page.keyboard.press('c')
    await expect(tool(page, 'Связь')).toHaveClass(/\bon\b/)
    await expect(page.locator('.hint-bar')).toBeVisible()
    await page.keyboard.press('Enter')
    await expect(a).toHaveAttribute('aria-label', /начало новой связи/)
    // to the card the arrow key reaches (a design has no link from the GitOps to the monitoring)
    await page.keyboard.press('ArrowLeft')
    const target = focusedCard(page)
    await expect(target).toHaveCount(1)
    expect(await target.getAttribute('data-id')).toBe('mon-dc2')
    await page.keyboard.press('Enter')
    await expect.poll(() => edges.count()).toBe(before + 1)
    await expect(a).not.toHaveAttribute('aria-label', /начало новой связи/)
    await page.keyboard.press('Escape')
    await expect(tool(page, 'Выбор')).toHaveClass(/\bon\b/)
  })

  test('Delete removes the card that has the focus, not the selection; the focus stays on the diagram; Ctrl+Z brings it back', async ({ page }) => {
    await createProject(page, uniqueName('kbd-delete'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    const cards = page.locator('.arch-node')
    const n = await cards.count()
    const selected = card(page, 'Kafka')
    await selected.focus()
    await page.keyboard.press('Enter')
    await expect(page.locator('.arch-node.selected')).toHaveCount(1)
    const other = card(page, 'Redis (master)')
    await other.focus()
    await page.keyboard.press('Delete')
    await expect(cards).toHaveCount(n - 1)
    await expect(card(page, 'Redis (master)')).toHaveCount(0)
    await expect(selected).toBeVisible() // the selected one is still there
    await expect(page.locator('.toast', { hasText: 'удалён' })).toBeVisible()
    expect(await page.evaluate(() => !!document.activeElement?.closest('.arch-node'))).toBe(true) // not on the page body
    await page.keyboard.press('Control+z')
    await expect(cards).toHaveCount(n)
  })
})

test.describe('every card has a name', () => {
  test('a button whose name begins with what is written on it, with what the picture says without words after it', async ({ page }) => {
    await createProject(page, uniqueName('kbd-names'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await settled(page)
    const check = async () => {
      const rows = await page.locator('.arch-node').evaluateAll((els) => els.map((e) => ({ role: e.getAttribute('role'), label: e.getAttribute('aria-label') ?? '', text: (e as HTMLElement).innerText.replace(/\s+/g, ' ').trim() })))
      expect(rows.length).toBeGreaterThan(10)
      for (const r of rows) {
        expect(r.role).toBe('button')
        expect(r.label.toLowerCase().startsWith(r.text.toLowerCase()), `"${r.label}" begins with "${r.text}"`).toBe(true)
      }
    }
    await check()
    // several instances are said in words
    await expect(page.locator('.arch-node.stack-2, .arch-node.stack-3').first()).toHaveAttribute('aria-label', /\d+ экземпляр/)
    // a finding is said in words, in the language of the page
    await card(page, 'PostgreSQL replica').click()
    await page.keyboard.press('Delete')
    const risky = page.locator('.arch-node.st-risk, .arch-node.st-critical').first()
    await expect(risky).toBeVisible({ timeout: 15_000 })
    await expect(risky).toHaveAttribute('aria-label', /риск|критично/i)
    await page.locator('.lang-switch button', { hasText: 'EN' }).click()
    await expect(page.locator('.arch-node.st-risk, .arch-node.st-critical').first()).toHaveAttribute('aria-label', /risk|critical/i)
    await check()
    await page.locator('.lang-switch button', { hasText: 'RU' }).click()
  })

  test('zones too: the head of a zone is a button named after the zone, and Enter selects it', async ({ page }) => {
    await createProject(page, uniqueName('kbd-zone'), '2 ЦОД + witness')
    await expect(page.locator('.zone-head').first()).toBeVisible()
    const head = page.locator('.zone-head').first()
    await expect(head).toHaveAttribute('role', 'button')
    const text = await head.locator('.zname').innerText()
    await expect(head).toHaveAttribute('aria-label', new RegExp('^' + text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')))
    await head.focus()
    await page.keyboard.press('Enter')
    await expect(head).toHaveAttribute('aria-pressed', 'true')
    await expect(page.locator('.inspector')).toBeVisible()
    await expect(page.locator('.inspector')).toContainText('Площадка')
  })
})

test.describe('the focused card is in view', () => {
  test.use({ viewport: { width: 1440, height: 900 } })

  test('forty arrow presses over a big design: the card with the focus is never outside the canvas or under what floats over it, and the zoom stays', async ({ page }) => {
    await createProject(page, uniqueName('kbd-view'), '10 площадок')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    // zoomed in, so that the design does not fit and the view has to follow the focus
    const zoom = () => page.locator('.zoom .pct').innerText()
    for (let i = 0; i < 12 && parseInt(await zoom(), 10) < 85; i++) await page.locator('.zoom').getByRole('button', { name: 'Приблизить' }).click()
    expect(parseInt(await zoom(), 10)).toBeGreaterThanOrEqual(80)
    const level = await zoom()
    await page.locator('.canvas [data-kbd][tabindex="0"]').focus()
    const visible = () => page.evaluate(() => {
      const el = document.activeElement?.closest('[data-kbd]')
      if (!el) return 'no focus on a card'
      const r = el.getBoundingClientRect()
      const c = document.querySelector('.canvas')!.getBoundingClientRect()
      if (r.left < c.left || r.top < c.top || r.right > c.right || r.bottom > c.bottom) return `outside the canvas: ${JSON.stringify(r)}`
      for (const o of document.querySelectorAll('[data-overlay]:not([data-overlay="float"])')) {
        const b = o.getBoundingClientRect()
        if (b.height && Math.min(r.right, b.right) - Math.max(r.left, b.left) > 1 && Math.min(r.bottom, b.bottom) - Math.max(r.top, b.top) > 1) return `under ${o.className}`
      }
      return 'ok'
    })
    const keys = ['ArrowRight', 'ArrowRight', 'ArrowDown', 'ArrowRight', 'ArrowDown', 'ArrowDown', 'ArrowLeft', 'ArrowDown']
    for (let i = 0; i < 40; i++) {
      await page.keyboard.press(keys[i % keys.length])
      await expect.poll(visible, { message: `after press ${i + 1} (${keys[i % keys.length]})` }).toBe('ok')
    }
    expect(await zoom()).toBe(level)
    // the canvas itself was not scrolled by the focus
    expect(await page.locator('.canvas').evaluate((e) => [e.scrollLeft, e.scrollTop])).toEqual([0, 0])
  })
})

test.describe('a failure on screen', () => {
  test.beforeEach(async ({ page }) => {
    expect((await page.request.put('/api/admin/settings', { data: { changes: { 'editor.failureLeadMs': 0 } } })).ok()).toBeTruthy()
  })
  test.afterEach(async ({ page }) => {
    await page.request.put('/api/admin/settings', { data: { changes: { 'editor.failureLeadMs': 2400 } } })
  })

  test('the arrow keys and Space on a card are the card\'s, they do not play or step the failure; the timeline still does', async ({ page }) => {
    await createProject(page, uniqueName('kbd-sim'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await page.locator('.whatif button', { hasText: 'ЦОД-1' }).click()
    const sim = page.getByTestId('simcard')
    await expect(sim).toBeVisible({ timeout: 15_000 })
    const step = sim.locator('.steps > li.cur')
    const last = await step.getAttribute('data-step')
    const a = card(page, 'Kafka')
    await a.focus()
    // the arrows walk from card to card; the step of the failure stays where it was
    await page.keyboard.press('ArrowLeft')
    await expect(a).not.toBeFocused()
    await expect(focusedCard(page)).toHaveCount(1)
    await expect(step).toHaveAttribute('data-step', last!)
    await page.keyboard.press('ArrowRight')
    await expect(step).toHaveAttribute('data-step', last!)
    // Space on a card selects it (it did not start the playback)
    await a.focus()
    await page.keyboard.press('Space')
    await expect(page.locator('.arch-node.selected')).toHaveCount(1)
    await expect(a).toHaveClass(/selected/)
    await expect(step).toHaveAttribute('data-step', last!)
    await expect(sim.getByTestId('play')).toHaveAttribute('aria-label', 'Воспроизвести')
    // the timeline keeps its keys
    await sim.locator('.track').focus()
    await page.keyboard.press('ArrowLeft')
    await expect(step).not.toHaveAttribute('data-step', last!)
    // what a card says in a failure: its state, in words
    await expect(page.locator('.arch-node[aria-label*="в показанном отказе"]').first()).toBeVisible()
  })
})

test.describe('links have no stop on the diagram, and are reached another way', () => {
  test('the palette finds a link by its two ends; the inspector of a card lists its links; the inspector of a link goes to its ends', async ({ page }) => {
    await createProject(page, uniqueName('kbd-links'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await expect(page.locator('.vue-flow__edge[tabindex]')).toHaveCount(0)
    const detail = await stored(page)
    const edge = detail.architecture.edges[0] as { id: string; source: string; target: string }
    const name = (id: string) => page.locator(`.arch-node[data-id="${id}"] .name`).innerText()
    const from = await name(edge.source)
    const to = await name(edge.target)

    await page.keyboard.press('Control+k')
    await page.locator('.palette input').fill(`${from} -> ${to}`)
    const hit = page.locator(`.palette [id="pal-edge:${edge.id}"]`)
    await expect(hit).toBeVisible()
    await page.keyboard.press('Enter')
    await expect(page.locator('.inspector')).toContainText('Связь')
    await expect(page.locator('.inspector [data-go="source"]')).toBeVisible()

    // from a link to its end: the card is selected, in view and has the keyboard
    await page.locator('.inspector [data-go="target"]').click()
    await expect(page.locator(`.arch-node[data-id="${edge.target}"]`)).toHaveClass(/selected/)
    await expect(page.locator(`.arch-node[data-id="${edge.target}"]`)).toBeFocused()
    // and the other way: the links of a card are buttons
    const links = page.getByTestId('inspector-link')
    expect(await links.count()).toBeGreaterThan(0)
    await links.first().click()
    await expect(page.locator('.inspector')).toContainText('Связь')
    await expect.poll(() => page.evaluate(() => !!document.activeElement?.closest('.inspector'))).toBe(true) // the keyboard went with it
    await page.keyboard.press('Escape')
    await expect(page.locator('.arch-node:focus')).toHaveCount(1)
  })
})

test.describe('the table of shortcuts is the dispatcher', () => {
  /** The keyboard is on the page and on nothing in particular (the keys of the page do not work over a control that takes them itself). */
  const free = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur())
  /** A key press as the Russian layout makes it: the letter is Cyrillic, the place of the key is the same. */
  const russian = (page: Page, key: string, code: string, mod = false) =>
    page.evaluate(([k, c, m]) => document.body.dispatchEvent(new KeyboardEvent('keydown', { key: k, code: c, ctrlKey: m, bubbles: true, cancelable: true })), [key, code, mod] as const)
  const pct = (page: Page) => page.locator('.zoom .pct').innerText().then((x) => parseInt(x, 10))
  const dialog = (page: Page) => page.getByRole('dialog')

  test('+ and - zoom, 0 fits, F is the focus button, L lays the cards out (Undo wakes), E exports, M opens the matrix', async ({ page }) => {
    await createProject(page, uniqueName('kbd-keys'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await settled(page)
    await free(page)
    const fitted = await pct(page)

    await page.keyboard.press('+')
    await expect.poll(() => pct(page)).toBeGreaterThan(fitted)
    const bigger = await pct(page)
    await page.keyboard.press('-')
    await page.keyboard.press('-')
    await expect.poll(() => pct(page)).toBeLessThan(bigger)
    await page.keyboard.press('0')
    await expect.poll(() => pct(page)).toBe(fitted)

    const focus = page.getByTestId('focus-toggle')
    const was = await focus.getAttribute('aria-pressed')
    await page.keyboard.press('f')
    await expect(focus).not.toHaveAttribute('aria-pressed', was!)
    await page.keyboard.press('f')
    await expect(focus).toHaveAttribute('aria-pressed', was!)

    const undo = page.locator('.zoom').getByRole('button', { name: 'Отменить' })
    await expect(undo).toBeDisabled()
    await page.keyboard.press('l')
    await expect(undo).toBeEnabled()

    await page.keyboard.press('e')
    await expect(dialog(page)).toContainText('Экспорт')
    // over a dialog the keys of the page wait: M does not open a second one, F does not flip the button
    await page.keyboard.press('m')
    await page.keyboard.press('f')
    await expect(dialog(page)).toHaveCount(1)
    await expect(dialog(page)).toContainText('Экспорт')
    await expect(focus).toHaveAttribute('aria-pressed', was!)
    await page.keyboard.press('Escape')
    await expect(dialog(page)).toHaveCount(0)

    await free(page)
    await page.keyboard.press('m')
    await expect(dialog(page)).toContainText('Матрица отказов')
  })

  test('the keys of the page are the keys of the place: they are not typed into a field and do not act over a button that takes them', async ({ page }) => {
    await createProject(page, uniqueName('kbd-typing'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    const search = page.locator('.library input')
    await search.focus()
    await page.keyboard.type('fem')
    await expect(search).toHaveValue('fem')
    await expect(dialog(page)).toHaveCount(0)
    const was = await page.getByTestId('focus-toggle').getAttribute('aria-pressed')
    await page.keyboard.type('f')
    await expect(page.getByTestId('focus-toggle')).toHaveAttribute('aria-pressed', was!)
    // a button has Space and Enter, and the single letters wait for it too
    await page.locator('.zoom').getByRole('button', { name: 'Приблизить' }).focus()
    await page.keyboard.press('e')
    await expect(dialog(page)).toHaveCount(0)
    // but the keys with a modifier are for every place
    await page.keyboard.press('Control+k')
    await expect(page.locator('.palette input')).toBeVisible()
    await page.keyboard.press('Escape')
  })

  test('on the Russian layout the letters are the same keys: С connects, М selects, Ctrl+Л opens the palette, Ctrl+Я undoes', async ({ page }) => {
    await createProject(page, uniqueName('kbd-ru'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await settled(page)
    await free(page)
    await russian(page, 'с', 'KeyC')
    await expect(tool(page, 'Связь')).toHaveClass(/\bon\b/)
    await russian(page, 'м', 'KeyV')
    await expect(tool(page, 'Выбор')).toHaveClass(/\bon\b/)
    await russian(page, 'л', 'KeyK', true)
    await expect(page.locator('.palette input')).toBeVisible()
    await page.keyboard.press('Escape')
    await expect(page.locator('.palette input')).toHaveCount(0)
    // an edit, and its undo by the key that is Ctrl+Z on that layout
    const undo = page.locator('.zoom').getByRole('button', { name: 'Отменить' })
    await russian(page, 'д', 'KeyL')
    await expect(undo).toBeEnabled()
    await russian(page, 'я', 'KeyZ', true)
    await expect(undo).toBeDisabled()
  })

  test('N and P go through the open findings, the worst first, and open the checks', async ({ page }) => {
    await createProject(page, uniqueName('kbd-findings'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    // an edit that makes findings: no replica of the database
    await card(page, 'PostgreSQL replica').click()
    await page.keyboard.press('Delete')
    await expect(page.locator('.arch-node.st-risk, .arch-node.st-critical').first()).toBeVisible({ timeout: 15_000 })
    await page.locator('.right .tab', { hasText: 'Требования' }).click()
    await free(page)
    await page.keyboard.press('n')
    await expect(page.locator('.right .tab.active')).toContainText('Проверки')
    await expect(page.locator('.arch-node.lit, .vue-flow__edge.lit, .arch-zone.lit').first()).toBeVisible()
    const first = await page.locator('.arch-node.lit').evaluateAll((els) => els.map((e) => e.getAttribute('data-id')))
    await page.keyboard.press('n')
    await page.keyboard.press('p')
    await expect.poll(() => page.locator('.arch-node.lit').evaluateAll((els) => els.map((e) => e.getAttribute('data-id')))).toEqual(first)
  })
})

test.describe('skip links', () => {
  test.use({ viewport: { width: 1920, height: 1080 } })

  test('from the start of the page the first Tab is a skip link, and Enter lands on the first card in reading order: two key presses (it was 436 Tab presses)', async ({ page }) => {
    await createProject(page, uniqueName('kbd-skip'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    // a page loaded afresh: nothing has the focus
    await page.goto(page.url())
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await settled(page)
    await page.keyboard.press('Tab')
    const skip = page.locator('.skip-link:focus')
    await expect(skip).toBeVisible()
    await expect(skip).toHaveText('К схеме')
    await page.keyboard.press('Enter')
    await expect(focusedCard(page)).toHaveClass(/arch-node/)
    // the card is in the first row of the diagram: no other card is higher by more than half a card
    const gap = await page.evaluate(() => {
      const top = (el: Element) => el.getBoundingClientRect().top
      const heights = Array.from(document.querySelectorAll('.arch-node')).map(top)
      const here = top(document.activeElement!)
      return { lower: here - Math.min(...heights), half: document.activeElement!.getBoundingClientRect().height / 2 }
    })
    expect(gap.lower).toBeLessThan(gap.half)
    await expect(page.locator('.canvas [data-kbd][tabindex="0"]')).toHaveCount(1)
  })

  test('the other two go to the checks and to the library', async ({ page }) => {
    await createProject(page, uniqueName('kbd-skip2'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await page.goto(page.url())
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await page.keyboard.press('Tab')
    await page.keyboard.press('Tab')
    await expect(page.locator('.skip-link:focus')).toHaveText('К проверкам')
    await page.keyboard.press('Enter')
    await expect(page.locator('.right [role="tab"][aria-selected="true"]')).toBeFocused()
    await page.locator('.skip-link', { hasText: 'К библиотеке' }).focus()
    await page.keyboard.press('Enter')
    await expect(page.locator('.library input')).toBeFocused()
    // folded, the checks are one button away
    await page.locator('[data-testid="right-fold"]').click()
    await page.locator('.skip-link', { hasText: 'К проверкам' }).focus()
    await page.keyboard.press('Enter')
    await expect(page.getByTestId('right-unfold')).toBeFocused()
  })

  test('an empty project has no card: the link to the diagram lands on the diagram itself', async ({ page }) => {
    await createProject(page, uniqueName('kbd-skip3'))
    await expect(page.locator('.canvas')).toBeVisible()
    await expect(page.locator('.arch-node')).toHaveCount(0)
    await page.goto(page.url())
    await expect(page.locator('.canvas')).toBeVisible()
    await page.keyboard.press('Tab')
    await expect(page.locator('.skip-link:focus')).toBeVisible()
    await page.keyboard.press('Enter')
    await expect(page.locator('.canvas')).toBeFocused()
  })
})

test.describe('the library is one stop of the Tab key', () => {
  test.use({ viewport: { width: 1920, height: 1080 } })
  const lib = '.library'
  const stops = (page: Page) => page.locator(`${lib} [tabindex="0"]`)

  test('Tab from the search leaves it in a few presses (it was 417); the arrows walk the headers and the tiles in the order of the page; Enter adds one card', async ({ page }) => {
    await createProject(page, uniqueName('kbd-lib'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await expect(page.locator(`${lib} .kind`).first()).toBeVisible()
    expect(await page.locator(`${lib} button`).count()).toBeGreaterThan(100) // the list is long
    await expect(stops(page)).toHaveCount(1)

    const search = page.locator(`${lib} input`)
    await search.focus()
    let presses = 0
    while (presses < 40) {
      await page.keyboard.press('Tab')
      presses++
      if (!(await page.evaluate(() => !!document.activeElement?.closest('.library')))) break
    }
    expect(presses).toBeLessThanOrEqual(6)

    // the arrows, Home and End walk the buttons in the order of the page
    const order = await page.locator(`${lib} .scroll button`).evaluateAll((els) => els.filter((e) => !(e as HTMLButtonElement).disabled).map((e) => e.textContent?.trim().slice(0, 30) ?? ''))
    const here = () => page.evaluate(() => Array.from(document.querySelectorAll('.library .scroll button')).filter((e) => !(e as HTMLButtonElement).disabled).indexOf(document.activeElement as Element))
    await stops(page).focus()
    const start = await here()
    await page.keyboard.press('ArrowDown')
    expect(await here()).toBe(start + 1)
    await page.keyboard.press('ArrowDown')
    await page.keyboard.press('ArrowUp')
    expect(await here()).toBe(start + 1)
    await page.keyboard.press('End')
    expect(await here()).toBe(order.length - 1)
    await page.keyboard.press('Home')
    expect(await here()).toBe(0)
    await expect(stops(page)).toHaveCount(1)
    await expect(stops(page)).toBeFocused() // the stop went with the focus

    // a header opens and closes with Right and Left
    const header = page.locator(`${lib} .ch[aria-expanded]`).first()
    await header.focus()
    const open = await header.getAttribute('aria-expanded')
    await page.keyboard.press(open === 'true' ? 'ArrowLeft' : 'ArrowRight')
    await expect(header).toHaveAttribute('aria-expanded', open === 'true' ? 'false' : 'true')
    await page.keyboard.press(open === 'true' ? 'ArrowRight' : 'ArrowLeft')
    await expect(header).toHaveAttribute('aria-expanded', open!)

    // Enter on a product adds exactly one card
    const cards = page.locator('.arch-node')
    const n = await cards.count()
    await page.locator(`${lib} .kind[data-kind]`).first().focus()
    await page.keyboard.press('Enter')
    await expect(cards).toHaveCount(n + 1)
  })

  test('the list is never left without its one stop: after "collapse all", after a search, after the search is cleared; "/" still goes to the search', async ({ page }) => {
    await createProject(page, uniqueName('kbd-lib2'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await expect(stops(page)).toHaveCount(1)
    await page.locator(`${lib} .bar .lnk`).nth(1).click() // collapse all
    await expect(stops(page)).toHaveCount(1)
    expect(await stops(page).evaluate((e) => e.tagName)).toBe('BUTTON')
    await page.locator(`${lib} .bar .lnk`).nth(0).click() // expand all
    await expect(stops(page)).toHaveCount(1)
    const search = page.locator(`${lib} input`)
    await search.fill('kafka')
    await expect(page.locator(`${lib} .kind`).first()).toBeVisible()
    await expect(stops(page)).toHaveCount(1)
    await search.fill('')
    await expect(stops(page)).toHaveCount(1)
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur())
    await page.keyboard.press('/')
    await expect(search).toBeFocused()
  })
})

test.describe('what an edit did to the verdict is said to a screen reader', () => {
  const region = (page: Page) => page.locator('body > [role="status"][aria-live="polite"][data-keep-live]')

  test('one polite region in the body: silent when a project opens or a card is moved, it speaks after an edit that changes the findings, in the language of the page, and a dialog does not shut it out', async ({ page }) => {
    await createProject(page, uniqueName('kbd-say'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await settled(page)
    await expect(region(page)).toHaveCount(1)
    // a project that is opened is not announced, however many findings it has
    await expect(page.locator('.crumbs .meta')).toContainText('сохранено', { timeout: 10_000 })
    await expect(region(page)).toHaveText('')

    // a card is moved: the engine is asked again and has nothing new to say
    const evaluated = page.waitForResponse((r) => r.request().method() === 'POST' && /\/evaluate$/.test(r.url()))
    const box = (await card(page, 'Kafka').boundingBox())!
    await page.mouse.move(box.x + 60, box.y + 30)
    await page.mouse.down()
    await page.mouse.move(box.x + 60 + 90, box.y + 30 + 40, { steps: 6 })
    await page.mouse.up()
    await evaluated
    await expect(page.locator('.crumbs .meta')).toContainText('сохранено', { timeout: 10_000 })
    await expect(region(page)).toHaveText('')

    // an edit that makes findings: no replica of the database
    await card(page, 'PostgreSQL replica').click()
    await page.keyboard.press('Delete')
    await expect.poll(() => region(page).innerText(), { timeout: 10_000 }).toMatch(/критич|риск/i)
    // the words name what is new and what it is about
    await expect.poll(() => region(page).innerText()).toMatch(/новы[ея]|новая/)
    // the region is not inside what a dialog makes inert
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur())
    await page.keyboard.press('e')
    await expect(page.getByRole('dialog')).toBeVisible()
    expect(await region(page).evaluate((e) => !!e.closest('[inert]'))).toBe(false)
    await page.keyboard.press('Escape')
    await expect(page.getByRole('dialog')).toHaveCount(0)

    // the edit is undone: the findings are gone, and that is said too
    await page.keyboard.press('Control+z')
    await expect.poll(() => region(page).innerText(), { timeout: 10_000 }).toMatch(/устранен/)

    // in English
    await page.locator('.lang-switch button', { hasText: 'EN' }).click()
    await card(page, 'PostgreSQL replica').click()
    await page.keyboard.press('Delete')
    await expect.poll(() => region(page).innerText(), { timeout: 10_000 }).toMatch(/new (critical|risk)/i)
    await page.locator('.lang-switch button', { hasText: 'RU' }).click()
  })
})
