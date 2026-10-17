import { expect } from '@playwright/test'
import { test } from './fixtures'

/**
 * The sign-in form says why it refused in the language of the page. The server's own answer is English ("invalid email or password"); before this the form
 * translated only `401` and `locked`, so the limit of an address (`rate_limited`) and an address the server would not look up (`invalid_email`) showed
 * server text to a Russian user. The mapping of every code is covered by `tests/authErrors.test.ts`; this is the form itself, with the real server.
 *
 * An address with no account is used: it is refused like a wrong password (one `401` for both) and locks nobody who exists.
 */
const STRANGER = 'nobody-e2e@example.com'

for (const [lang, said] of [
  ['ru', 'Неверный email или пароль'],
  ['en', 'Wrong email or password'],
] as const) {
  test(`a refused sign-in is a sentence of the dictionary, not the server's text (${lang})`, async ({ page }) => {
    await page.goto('/login')
    await page.evaluate((l) => localStorage.setItem('ia_lang', l), lang)
    await page.reload()
    await page.fill('#email', STRANGER)
    await page.fill('#password', 'not the password at all')
    await page.click('button.btn-primary')
    const banner = page.locator('.banner-err')
    await expect(banner).toHaveText(said)
    await expect(banner).toHaveAttribute('role', 'alert')
    await expect(page).toHaveURL(/\/login/)
  })
}
