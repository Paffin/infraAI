import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { expect, type Page } from '@playwright/test'
import type { Architecture } from '../src/api/types'
import { test } from './fixtures'
import { login, projectWith, uniqueName } from './helpers'

// What the server refuses is said in the language of the person. The server answers with a stable `code`, the numbers in `details` and an English sentence for scripts;
// the page puts the code in its own words. The refusals here are the real server's (the limits and the formats of the registry are the product's), read in both languages.

const recorded = JSON.parse(readFileSync(join(process.cwd(), 'tests', 'fixtures', 'plan-spof.json'), 'utf8')) as { architecture: Architecture }

const saveButton = (page: Page) => page.locator('.savebar .btn-primary, .adm-main .btn-primary', { hasText: /Сохранить|Save/ }).first()

/** Types `value` into the help link of the settings page, saves, and returns what the server answered. */
async function refusedHelpLink(page: Page, value: string) {
  await page.goto('/admin/settings/general')
  await page.locator('[id="set-ui.helpUrl"]').fill(value)
  const answered = page.waitForResponse((r) => r.request().method() === 'PUT' && r.url().endsWith('/api/admin/settings'))
  await saveButton(page).click()
  const res = await answered
  expect(res.status()).toBe(400)
  return (await res.json()) as { error: { code: string; message: string; details: { key: string; reasonCode: string } } }
}

test.describe('a setting the server refuses', () => {
  test('is explained in Russian: under the field, in the line of the form and in the toast', async ({ page }) => {
    await login(page)
    const body = await refusedHelpLink(page, 'javascript:alert(document.domain)')
    // what the server sent: the code the page puts in words, and its own English sentence, which the page does not show
    expect([body.error.code, body.error.details.key, body.error.details.reasonCode]).toEqual(['invalid_setting', 'ui.helpUrl', 'not_http'])
    expect(body.error.message).toContain('must be an address')

    const row = page.locator('[id="setting-ui.helpUrl"]')
    await expect(row.locator('.srow-err')).toHaveText('Нужен адрес, который начинается с http:// или https://')
    await expect(page.locator('.toast.error').first()).toContainText('ui.helpUrl: Нужен адрес, который начинается с http:// или https://')
    await expect(page.locator('body')).not.toContainText('must be an address')
  })

  test('is explained in English when the page is in English, and a bound carries its number', async ({ page }) => {
    await page.addInitScript(() => localStorage.setItem('ia_lang', 'en'))
    await login(page)
    await refusedHelpLink(page, 'javascript:alert(document.domain)')
    await expect(page.locator('[id="setting-ui.helpUrl"] .srow-err')).toHaveText('An address that starts with http:// or https:// is required')

    // an address with a user and a password in it: another reason, another sentence
    await refusedHelpLink(page, 'https://admin:secret@docs.example.com/')
    await expect(page.locator('[id="setting-ui.helpUrl"] .srow-err')).toHaveText('The address must not contain a user name or a password')
  })
})

test.describe('a text past its limit', () => {
  test('says which field and how many characters it may have, in Russian', async ({ page }) => {
    test.setTimeout(120_000)
    await login(page)
    const all = ((await (await page.request.get('/api/admin/settings')).json()) as { settings: { key: string; value: unknown }[] }).settings
    const was = all.find((s) => s.key === 'limits.maxTextChars')!.value
    // the smallest bound the registry allows: the field of the form takes 200 characters, so a longer message than 100 reaches the server
    expect((await page.request.put('/api/admin/settings', { data: { changes: { 'limits.maxTextChars': 100 } } })).ok()).toBe(true)
    try {
      const id = await projectWith(page, uniqueName('refusals'), recorded.architecture)
      await page.goto(`/p/${id}`)
      await expect(page.locator('.arch-node').first()).toBeVisible()
      await page.locator('button[title="Версии · diff"]').click()
      await page.locator('.vers .save input').fill('x'.repeat(150))
      const answered = page.waitForResponse((r) => r.request().method() === 'POST' && r.url().endsWith(`/api/projects/${id}/versions`))
      await page.locator('.vers .save .btn-primary').click()
      const res = await answered
      expect(res.status()).toBe(400)
      const body = (await res.json()) as { error: { code: string; message: string; details: unknown } }
      expect(body.error.code).toBe('field_too_long')
      expect(body.error.details).toEqual({ field: 'message', max: 100, actual: 150 })
      await expect(page.locator('.toast.error').first()).toHaveText('Поле «Сообщение» слишком длинное: не больше 100 знаков, сейчас 150')
      await expect(page.locator('.toast.error').first()).not.toContainText('longer than')
    } finally {
      await page.request.put('/api/admin/settings', { data: { changes: { 'limits.maxTextChars': was } } })
    }
  })
})
