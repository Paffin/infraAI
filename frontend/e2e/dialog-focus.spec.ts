import { expect, type Locator, type Page } from '@playwright/test'
import { madeByApi, test } from './fixtures'
import { createProject, login, settled, uniqueName } from './helpers'

/**
 * A dialog takes the keyboard, holds it and gives it back (lib/useFocusScope.ts). Every row opens one dialog the way a keyboard user does (focus the control,
 * press Enter), then checks, in a real browser:
 *   - the focus is inside the dialog at once;
 *   - 40 Tab and 40 Shift+Tab presses never leave it, and together they visit exactly the controls the dialog has (a dialog with one control would pass the first
 *     check trivially);
 *   - the page behind and every other dialog are inert while it is open, the toasts are not, and nothing is inert after;
 *   - Escape closes it and the focus is on the control that opened it.
 *
 * Measured before the change (Playwright on the old Modal): 0 of 60 Tab presses entered the Export and the Versions dialogs, the new-project dialog kept 17 of 40
 * (the rest went round the page behind), the rename prompt 6 of 40; one Escape closed the confirm AND the Versions window under it; Escape on the palette
 * left the focus on the body.
 */
const DIALOG = '[role="dialog"][aria-modal="true"]'
const TEMPLATE = '2 ЦОД + witness'

interface Row {
  name: string
  screen: 'projects' | 'editor' | 'empty-editor'
  /** The control that has the focus when the dialog is opened, and has it again when it is closed. */
  returnTo: (page: Page, project: string) => Locator
  /** Opens the dialog from the keyboard. */
  open: (page: Page, project: string) => Promise<void>
  /** The dialog is one control (the palette): Tab keeps the focus on it. */
  hold?: boolean
  /** The dialog fills itself in after it opens (a comparison that is computed): what to wait for before its controls are counted. */
  ready?: (page: Page) => Promise<void>
}

const card = (page: Page, project: string) => page.locator('article.project', { hasText: project })
const header = (page: Page) => page.locator('header.eh')

/** Focuses the control the way Tab would have, and presses Enter on it. */
async function press(control: Locator, key = 'Enter') {
  await control.focus()
  await control.page().keyboard.press(key)
}

const ROWS: Row[] = [
  { name: 'new project', screen: 'projects', returnTo: (p) => p.getByRole('button', { name: 'Новый проект' }).first(), open: (p) => press(p.getByRole('button', { name: 'Новый проект' }).first()) },
  { name: 'rename prompt', screen: 'projects', returnTo: (p, n) => card(p, n).getByRole('button', { name: 'Переименовать' }), open: (p, n) => press(card(p, n).getByRole('button', { name: 'Переименовать' })) },
  { name: 'delete confirm', screen: 'projects', returnTo: (p, n) => card(p, n).getByRole('button', { name: 'Удалить' }), open: (p, n) => press(card(p, n).getByRole('button', { name: 'Удалить' })) },
  { name: 'export', screen: 'editor', returnTo: (p) => header(p).locator('[data-tour="export"]'), open: (p) => press(header(p).locator('[data-tour="export"]')) },
  { name: 'versions', screen: 'editor', returnTo: (p) => header(p).locator('button[title="Версии · diff"]'), open: (p) => press(header(p).locator('button[title="Версии · diff"]')) },
  { name: 'failure matrix', screen: 'editor', returnTo: (p) => header(p).locator('button[title="Симуляция отказов"]'), open: (p) => press(header(p).locator('button[title="Симуляция отказов"]')) },
  { name: 'score', screen: 'editor', returnTo: (p) => header(p).locator('.score'), open: (p) => press(header(p).locator('.score')) },
  // the "?" key, pressed while a control has the focus
  { name: 'shortcuts', screen: 'editor', returnTo: (p) => header(p).locator('.score'), open: async (p) => { await header(p).locator('.score').focus(); await p.keyboard.press('?') } },
  { name: 'placement wizard', screen: 'empty-editor', ready: (p) => expect(p.locator('.wiz .cmp tbody tr').first()).toBeVisible({ timeout: 30_000 }), returnTo: (p) => p.getByTestId('canvas-empty').locator('.start', { hasText: 'Мастер размещения' }), open: (p) => press(p.getByTestId('canvas-empty').locator('.start', { hasText: 'Мастер размещения' })) },
  // through the Help menu: the item that was pressed is gone with the menu, the dialog must still find its way back to the menu's button
  {
    name: 'about', screen: 'editor', returnTo: (p) => p.locator('.help-menu .trigger'),
    open: async (p) => { await press(p.locator('.help-menu .trigger')); await press(p.getByTestId('help-about')) },
  },
  { name: 'palette', screen: 'editor', hold: true, returnTo: (p) => header(p).locator('.score'), open: async (p) => { await header(p).locator('.score').focus(); await p.keyboard.press('Control+k') } },
]

// ---- what the page says about itself ------------------------------------------------------------------------------------------------------------------
/** Where the focus is: inside a modal dialog, and which element (a number that stays the same for the same element). */
async function where(page: Page) {
  return page.evaluate((sel) => {
    const w = window as unknown as { __fx?: WeakMap<Element, number>; __fxn?: number }
    w.__fx ??= new WeakMap()
    w.__fxn ??= 0
    const a = document.activeElement
    if (!a || a === document.body) return { inside: false, id: 0, tag: 'body', scroller: false }
    if (!w.__fx.has(a)) w.__fx.set(a, ++w.__fxn)
    // since Chrome 130 a scrollable area with nothing to press inside is a Tab stop of its own, so that the keyboard can scroll it
    const scroller = a.scrollHeight > a.clientHeight || a.scrollWidth > a.clientWidth
    return { inside: !!a.closest(sel), id: w.__fx.get(a)!, tag: `${a.tagName.toLowerCase()}${a.id ? '#' + a.id : ''}.${a.className}`, scroller }
  }, DIALOG)
}

/** The controls of the top dialog that a Tab key reaches: found here the way the browser's own rules say (visible, not disabled, not tabindex<0, one stop per radio group). */
async function tabStops(page: Page): Promise<number[]> {
  return page.evaluate((sel) => {
    const w = window as unknown as { __fx?: WeakMap<Element, number>; __fxn?: number }
    w.__fx ??= new WeakMap()
    w.__fxn ??= 0
    const dialogs = Array.from(document.querySelectorAll(sel))
    const dlg = dialogs[dialogs.length - 1]
    if (!dlg) return []
    const all = Array.from(dlg.querySelectorAll<HTMLElement>('a[href], button, input:not([type="hidden"]), select, textarea, summary, [contenteditable]:not([contenteditable="false"]), [tabindex]'))
    const stops = all.filter((e) => !(e as HTMLButtonElement).disabled && !(e.getAttribute('tabindex') !== null && Number(e.getAttribute('tabindex')) < 0) && e.checkVisibility({ visibilityProperty: true }) && !e.closest('[inert]'))
    const radios = new Map<string, HTMLInputElement[]>()
    for (const e of stops) if (e instanceof HTMLInputElement && e.type === 'radio') radios.set(e.name, [...(radios.get(e.name) ?? []), e])
    const dropped = new Set<Element>()
    for (const group of radios.values()) { const keep = group.find((r) => r.checked) ?? group[0]; for (const r of group) if (r !== keep) dropped.add(r) }
    return stops.filter((e) => !dropped.has(e)).map((e) => { if (!w.__fx!.has(e)) w.__fx!.set(e, ++w.__fxn!); return w.__fx!.get(e)! })
  }, DIALOG)
}

/** Every child of the body that is not a script or a style: is it inert, does it hold the dialog, does it stay live (the toasts). */
async function bodyChildren(page: Page) {
  return page.evaluate((sel) => Array.from(document.body.children)
    .filter((c) => !['SCRIPT', 'STYLE', 'LINK', 'NOSCRIPT', 'TEMPLATE'].includes(c.tagName))
    .map((c) => ({ name: `${c.tagName.toLowerCase()}${c.id ? '#' + c.id : ''}${c.className ? '.' + String(c.className).split(' ')[0] : ''}`, inert: c.hasAttribute('inert'), hasDialog: !!c.querySelector(sel) || c.matches(sel), live: c.hasAttribute('data-keep-live') })), DIALOG)
}

/** The dialogs' controls come in as the report and the lists load: wait until the set of Tab stops stops changing. */
async function stable(page: Page) {
  await settled(page)
  let last = -1
  let same = 0
  for (let i = 0; i < 60 && same < 3; i++) {
    const n = (await tabStops(page)).length
    same = n === last ? same + 1 : 0
    last = n
    await page.waitForTimeout(120)
  }
}

async function createProjectNamed(page: Page, name: string) {
  const r = await page.request.post('/api/projects', { data: { name } })
  expect(r.ok()).toBe(true)
  madeByApi.push((await r.json()).id)
}

test.describe('dialogs take, hold and return the keyboard', () => {
  for (const row of ROWS) {
    test(row.name, async ({ page }) => {
      const project = uniqueName('focus')
      await login(page)
      if (row.screen === 'projects') {
        await createProjectNamed(page, project)
        await page.reload()
        await expect(card(page, project)).toBeVisible()
      } else if (row.screen === 'empty-editor') {
        await createProject(page, project)
        await expect(page.getByTestId('canvas-empty')).toBeVisible()
      } else {
        await createProject(page, project, TEMPLATE)
        await expect(page.locator('.arch-node').first()).toBeVisible()
      }
      await settled(page)

      const back = row.returnTo(page, project)
      await back.evaluate((el) => el.setAttribute('data-fx-return', ''))
      await row.open(page, project)

      // 1. the focus is in the dialog right away
      const dlg = page.locator(DIALOG)
      await expect(dlg).toHaveCount(1)
      await expect(dlg).toBeVisible()
      await expect.poll(async () => (await where(page)).inside, { message: 'the focus is inside the dialog after it opened' }).toBe(true)
      await row.ready?.(page)
      await stable(page)

      // 2. forty presses each way never leave; together they visit exactly the controls the dialog has
      const stops = await tabStops(page)
      expect(stops.length, 'a dialog with something to press').toBeGreaterThan(0)
      const presses = Math.max(40, stops.length + 3)
      const visited = new Map<number, { scroller: boolean; tag: string }>()
      const first = await where(page)
      visited.set(first.id, first)
      for (const key of ['Tab', 'Shift+Tab']) {
        for (let i = 0; i < presses; i++) {
          await page.keyboard.press(key)
          const now = await where(page)
          expect(now.inside, `${key} press ${i + 1} left the dialog for ${now.tag}`).toBe(true)
          visited.set(now.id, now)
        }
      }
      if (row.hold) {
        expect([...visited.keys()], 'one control, the focus stays on it').toHaveLength(1)
      } else {
        for (const id of stops) expect(visited.has(id), `a control of the dialog was never reached (id ${id})`).toBe(true)
        // what else was reached can only be an area that scrolls (a Tab stop of the browser's own), never something outside the dialog
        for (const [id, v] of visited) if (!stops.includes(id)) expect(v.scroller, `${v.tag} was reached by Tab and is not one of the dialog's controls`).toBe(true)
        // Shift+Tab from the first control lands on the last, Tab from the last lands on the first
        for (let i = 0; i <= stops.length + 1 && (await where(page)).id !== stops[0]; i++) await page.keyboard.press('Tab')
        expect((await where(page)).id).toBe(stops[0])
        await page.keyboard.press('Shift+Tab')
        expect((await where(page)).id, 'Shift+Tab from the first control').toBe(stops[stops.length - 1])
        await page.keyboard.press('Tab')
        expect((await where(page)).id, 'Tab from the last control').toBe(stops[0])
      }

      // 3. the page and the dialogs below are inert while it is open; the toasts are not
      for (const c of await bodyChildren(page)) {
        if (c.hasDialog || c.live) expect(c.inert, `${c.name} must be free`).toBe(false)
        else expect(c.inert, `${c.name} must be inert while the dialog is open`).toBe(true)
      }

      // 4. Escape closes it and the focus goes back to the control that opened it
      await page.keyboard.press('Escape')
      await expect(dlg).toHaveCount(0)
      await expect.poll(() => page.evaluate(() => document.activeElement?.hasAttribute('data-fx-return') ?? false), { message: 'the focus is back on the opener' }).toBe(true)
      for (const c of await bodyChildren(page)) expect(c.inert, `${c.name} must not stay inert`).toBe(false)
    })
  }
})

test.describe('dialogs over dialogs', () => {
  test('Versions → Restore → confirm: one Escape closes the question only, the next closes Versions, each returns the focus where it came from', async ({ page }) => {
    await login(page)
    await createProject(page, uniqueName('stack'), TEMPLATE)
    const versions = header(page).locator('button[title="Версии · diff"]')
    await versions.evaluate((el) => el.setAttribute('data-fx-return', 'versions'))
    await press(versions)
    await expect(page.locator(DIALOG)).toHaveCount(1)
    const restore = page.locator('.vers .list li').first().getByRole('button', { name: 'Восстановить' })
    await expect(restore).toBeVisible()
    await restore.evaluate((el) => el.setAttribute('data-fx-return', 'restore'))
    await press(restore)
    await expect(page.locator(DIALOG)).toHaveCount(2)
    const dialogs = await page.evaluate((sel) => Array.from(document.querySelectorAll(sel)).map((d) => d.closest('.modal-backdrop')?.hasAttribute('inert') ?? false), DIALOG)
    expect(dialogs, 'the dialog under the question is inert').toEqual([true, false])
    // a question that destroys something opens on "Cancel"
    await expect(page.locator(DIALOG).last().locator('.modal-foot .btn').first()).toBeFocused()

    await page.keyboard.press('Escape')
    await expect(page.locator(DIALOG)).toHaveCount(1) // before: 2 -> 0
    await expect.poll(() => page.evaluate(() => document.activeElement?.getAttribute('data-fx-return'))).toBe('restore')
    const page_ = await bodyChildren(page)
    expect(page_.find((c) => c.name.startsWith('div#app'))?.inert, 'the page stays locked under Versions').toBe(true)

    await page.keyboard.press('Escape')
    await expect(page.locator(DIALOG)).toHaveCount(0)
    await expect.poll(() => page.evaluate(() => document.activeElement?.getAttribute('data-fx-return'))).toBe('versions')
  })

  test('the palette over a dialog: one Escape closes the palette, not the dialog under it', async ({ page }) => {
    await login(page)
    await createProject(page, uniqueName('pal'), TEMPLATE)
    await press(header(page).locator('[data-tour="export"]'))
    await expect(page.locator(DIALOG)).toHaveCount(1)
    await expect.poll(async () => (await where(page)).inside).toBe(true)
    await page.keyboard.press('Control+k')
    await expect(page.locator(DIALOG)).toHaveCount(2)
    await expect(page.locator('.palette input')).toBeFocused()
    await page.keyboard.press('Escape')
    await expect(page.locator(DIALOG)).toHaveCount(1) // before: the dialog closed and the palette stayed
    await expect(page.locator('.palette')).toHaveCount(0)
    await page.keyboard.press('Escape')
    await expect(page.locator(DIALOG)).toHaveCount(0)
  })

  test('a command of the palette that opens a dialog: Escape returns to the control that was focused before the palette', async ({ page }) => {
    await login(page)
    await createProject(page, uniqueName('cmd'), TEMPLATE)
    const score = header(page).locator('.score')
    await score.evaluate((el) => el.setAttribute('data-fx-return', ''))
    await score.focus()
    await page.keyboard.press('Control+k')
    const input = page.locator('.palette input')
    await input.fill('Экспорт: Terraform')
    // the arrow keys to the command (what is found first depends on the diagram: a node named like it may come before)
    for (let i = 0; i < 40 && (await input.getAttribute('aria-activedescendant')) !== 'pal-panel:export'; i++) await page.keyboard.press('ArrowDown')
    await expect(input).toHaveAttribute('aria-activedescendant', 'pal-panel:export')
    await page.keyboard.press('Enter')
    await expect(page.locator(DIALOG)).toHaveCount(1)
    await expect(page.locator('.palette')).toHaveCount(0)
    await expect.poll(async () => (await where(page)).inside).toBe(true)
    await page.keyboard.press('Escape')
    await expect(page.locator(DIALOG)).toHaveCount(0)
    await expect.poll(() => page.evaluate(() => document.activeElement?.hasAttribute('data-fx-return') ?? false)).toBe(true)
  })

  test('a message raised while a dialog is open stays readable and clickable over it', async ({ page }) => {
    await login(page)
    await createProject(page, uniqueName('toast'), TEMPLATE)
    await press(header(page).locator('button[title="Версии · diff"]'))
    await expect(page.locator(DIALOG)).toHaveCount(1)
    await page.locator(`${DIALOG} .btn-primary`, { hasText: 'Сохранить' }).first().click()
    const toast = page.locator('.toast').first()
    await expect(toast).toBeVisible()
    expect(await toast.evaluate((el) => !el.closest('[inert]')), 'a toast is not inside the inert page').toBe(true)
    await toast.click() // an inert element does not take clicks: this would time out
    await expect(page.locator('.toast')).toHaveCount(0)
    await expect(page.locator(DIALOG)).toHaveCount(1) // the dialog is still there, and still holds the keyboard: a Tab lands inside it
    await page.keyboard.press('Tab')
    await expect.poll(async () => (await where(page)).inside).toBe(true)
  })
})

test.describe('menus return the keyboard', () => {
  test('Escape in the Help menu, with the focus on one of its items, puts the focus back on the menu button', async ({ page }) => {
    await login(page)
    await createProject(page, uniqueName('menu'), TEMPLATE)
    const trigger = page.locator('.help-menu .trigger')
    await press(trigger)
    await expect(page.locator('.help-menu .menu')).toBeVisible()
    await page.keyboard.press('Tab')
    expect(await page.evaluate(() => !!document.activeElement?.closest('.help-menu .menu')), 'Tab went into the menu').toBe(true)
    await page.keyboard.press('Escape')
    await expect(page.locator('.help-menu .menu')).toHaveCount(0)
    await expect(trigger).toBeFocused()
  })

  test('a menu item that opens a dialog: closing the dialog returns to the menu button, not to the page top', async ({ page }) => {
    await login(page)
    await createProject(page, uniqueName('menu2'), TEMPLATE)
    const trigger = page.locator('.help-menu .trigger')
    await press(trigger)
    await page.getByRole('button', { name: 'Горячие клавиши' }).click() // with the mouse: the item is pressed, not focused through Tab
    await expect(page.locator(DIALOG)).toHaveCount(1)
    await page.keyboard.press('Escape')
    await expect(page.locator(DIALOG)).toHaveCount(0)
    await expect(trigger).toBeFocused()
  })
})
