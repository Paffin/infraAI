import { expect, type Page } from '@playwright/test'
import { test } from './fixtures'
import { createProject, login, settled, uniqueName } from './helpers'

/**
 * A server with no AI (no OpenRouter key, or AI switched off by the administrator; the stack of CI has no key) leaves nothing in the editor that cannot work: no prompt
 * bar (it was 67 px of the canvas, disabled), no switch between "AI" and "manual", no AI chat tab, no palette commands for them, no request for the chat. The one who can
 * fix it, the administrator, gets a single hint with the way to the setting. (With a key everything is there: that side is in tests/aiChrome.test.ts, on the real stores.)
 *
 * Measured before the change, on this stack: `.promptbar` 67 px, canvas top 125 px under a header of 58, two buttons in `.mode`, four tabs on the right (the last "ИИ-чат"),
 * "Открыть: ИИ-чат" and "Режим: …" in the palette, a request to `/projects/:id/chat` on every opening, "опишите систему ИИ" in the empty diagram and in the new-project dialog.
 */
const TEMPLATE = '2 ЦОД + witness'

async function aiOnServer(page: Page): Promise<boolean> {
  return (await (await page.request.get('/api/bootstrap')).json()).ai.enabled === true
}

test.describe('a server with no AI', () => {
  test('a user who edits sees no AI controls, and the canvas starts right under the header', async ({ page, browser, baseURL }) => {
    await login(page)
    test.skip(await aiOnServer(page), 'this stack has an AI key: the controls are there, rightly')

    // an editor, made by the administrator, with a password of their own (so that there is nothing to change at the first sign-in)
    const stamp = Date.now().toString(36)
    const email = `ai-off-${stamp}@e2e.local`
    const password = 'editor password 12345'
    const made = await page.request.post('/api/admin/users', { data: { email, role: 'editor', password } })
    expect(made.ok()).toBe(true)
    const userId = (await made.json()).id as string

    const context = await browser.newContext({ baseURL, locale: 'ru-RU', viewport: { width: 1920, height: 1080 } })
    const editor = await context.newPage()
    let projectId = ''
    try {
      await editor.addInitScript(() => { try { localStorage.setItem('ia_tour_off', '1') } catch { /* no storage: no tour either */ } })
      await login(editor, { email, password })

      // the dialog of a new project does not promise an AI either
      await editor.getByRole('button', { name: 'Новый проект' }).first().click()
      await expect(editor.getByTestId('tpl-blank')).toBeVisible()
      await expect(editor.getByTestId('tpl-blank')).not.toContainText('ИИ')
      await editor.keyboard.press('Escape')

      const chat: string[] = []
      editor.on('request', (r) => { if (/\/api\/projects\/[^/]+\/chat/.test(r.url())) chat.push(r.url()) })
      await createProject(editor, uniqueName('aioff'))
      projectId = editor.url().split('/p/')[1]
      await expect(editor.getByTestId('canvas-empty')).toBeVisible()
      await settled(editor)

      await expect(editor.locator('.promptbar')).toHaveCount(0)
      await expect(editor.locator('header.eh .mode')).toHaveCount(0)
      await expect(editor.locator('.right .tab')).toHaveText(['Проверки', 'Требования', 'Анализ']) // no "ИИ-чат"
      await expect(editor.locator('a[href="/admin/llm"]')).toHaveCount(0) // the hint is for whoever can act on it

      // the canvas is right under the header (before: header 58 + prompt bar 67 = 125)
      const header = (await editor.locator('header.eh').boundingBox())!
      const canvas = (await editor.locator('.canvas').boundingBox())!
      const gap = canvas.y - (header.y + header.height)
      expect(gap, `the canvas starts ${gap}px under the header`).toBeLessThanOrEqual(2)

      // the empty diagram sends nobody to a prompt, and names the library while it is open and only then
      const empty = editor.getByTestId('canvas-empty')
      const words = empty.locator('p').first()
      await expect(empty).not.toContainText('ИИ')
      await expect(words).toContainText('библиотек')
      await expect(words).toContainText('слева')
      await editor.keyboard.press('BracketLeft') // fold the library
      await expect(editor.locator('.library')).toBeHidden()
      await expect(empty).not.toContainText('ИИ')
      // the words no longer name a panel that is not on the screen (the button for the templates does: it opens that panel)
      await expect(words).not.toContainText('слева')
      await expect(words).not.toContainText('библиотек')

      // nothing in the palette opens the chat or switches the AI
      await editor.keyboard.press('Control+k')
      const input = editor.locator('.palette input')
      for (const query of ['чат', 'вручную', 'ИИ-чат', 'режим']) {
        await input.fill(query)
        const titles = await editor.locator('.palette .item .ti').allInnerTexts()
        // the chat tab and the two modes; a product with "AI" in its name (a command to add it) is a component of the catalog, not the AI of the service
        expect(titles.filter((x) => /ИИ-чат|^Режим:/.test(x)), `commands found for «${query}»`).toEqual([])
      }
      await editor.keyboard.press('Escape')

      // a request that could only answer "nothing" is not made
      expect(chat, 'requests for the chat').toEqual([])
    } finally {
      if (projectId) await editor.request.delete(`/api/projects/${projectId}`).catch(() => undefined)
      await context.close()
      await page.request.patch(`/api/admin/users/${userId}`, { data: { disabled: true } }).catch(() => undefined)
    }
  })

  test('the administrator gets one hint with the way to the setting; hiding it is remembered', async ({ page }) => {
    // this test wants to see the hint: the fixture's "already hidden" is taken back, once (a reload below must not take it back again)
    await page.addInitScript(() => {
      try {
        if (!sessionStorage.getItem('fx-hint-reset')) { localStorage.removeItem('ia_ai_hint_off'); sessionStorage.setItem('fx-hint-reset', '1') }
      } catch { /* no storage: nothing to take back */ }
    })
    await login(page)
    test.skip(await aiOnServer(page), 'this stack has an AI key: there is nothing to hint at')
    await createProject(page, uniqueName('hint'), TEMPLATE)

    const hint = page.getByTestId('ai-off-hint')
    await expect(hint).toBeVisible()
    await expect(page.locator('a[href="/admin/llm"]')).toHaveCount(1)
    await expect(page.locator('.promptbar')).toHaveCount(0)

    await hint.getByRole('button', { name: 'Скрыть подсказку' }).click()
    await expect(hint).toHaveCount(0)
    await expect(page.locator('a[href="/admin/llm"]')).toHaveCount(0)
    expect(await page.evaluate(() => localStorage.getItem('ia_ai_hint_off'))).toBe('1')
    await page.reload()
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await expect(page.getByTestId('ai-off-hint')).toHaveCount(0)
  })

  test('the link of the hint leads to the AI settings', async ({ page }) => {
    await page.addInitScript(() => {
      try {
        if (!sessionStorage.getItem('fx-hint-reset')) { localStorage.removeItem('ia_ai_hint_off'); sessionStorage.setItem('fx-hint-reset', '1') }
      } catch { /* no storage: nothing to take back */ }
    })
    await login(page)
    test.skip(await aiOnServer(page), 'this stack has an AI key: there is nothing to hint at')
    await createProject(page, uniqueName('hintgo'), TEMPLATE)
    await page.getByTestId('ai-off-hint').getByRole('link').click()
    await expect(page).toHaveURL(/\/admin\/llm$/)
    await expect(page).toHaveTitle(/OpenRouter/)
  })

  test('the tour of the editor walks past the missing prompt bar and ends', async ({ page }) => {
    await page.addInitScript(() => { try { localStorage.removeItem('ia_tour_off') } catch { /* ignore */ } })
    await login(page)
    test.skip(await aiOnServer(page), 'this stack has an AI key: the tour stops at the prompt bar, rightly')
    const tour = page.locator('[data-testid="tour"]')
    await expect(tour).toBeVisible({ timeout: 15_000 })
    await page.keyboard.press('Escape') // the tour of the project list
    await createProject(page, uniqueName('tourai'), TEMPLATE)
    await expect(tour).toBeVisible({ timeout: 15_000 })
    const seen: string[] = [await tour.locator('h3').innerText()]
    for (let i = 0; i < 14 && (await tour.getByTestId('tour-next').innerText()) !== 'Готово'; i++) {
      const before = seen[seen.length - 1]
      await tour.getByTestId('tour-next').click()
      await expect(tour.locator('h3')).not.toHaveText(before) // the card swaps its words with a transition
      seen.push(await tour.locator('h3').innerText())
    }
    expect(seen).not.toContain('Опишите словами')
    expect(seen[seen.length - 1]).toBe('Готово')
    await tour.getByTestId('tour-next').click()
    await expect(tour).toHaveCount(0)
  })
})
