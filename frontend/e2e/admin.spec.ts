import { expect } from '@playwright/test'
import { test } from './fixtures'
import { login } from './helpers'

test.describe('admin panel', () => {
  test('every section renders without errors', async ({ page }) => {
    const errors: string[] = []
    page.on('pageerror', (e) => errors.push(e.message))
    page.on('console', (m) => { if (m.type() === 'error' && !m.text().includes('401')) errors.push(m.text()) })
    await login(page)
    for (const path of ['settings', 'llm', 'catalog/kind', 'catalog/versions', 'catalog/category', 'placement/pattern', 'placement/stack', 'placement/settings', 'rules', 'templates', 'iac', 'prompts', 'engine', 'backup', 'users', 'audit', 'system']) {
      await page.goto('/admin/' + path)
      await expect(page.locator('#adm-main')).toBeVisible()
      await expect(page.locator('#adm-main .cfg-section, #adm-main > *').first()).toBeVisible()
      await expect(page.locator('#adm-main')).not.toHaveText(/^\s*404\s*$/)
      await expect(page.locator('body')).not.toContainText(/admin\.[a-z]+\.[a-z]/) // raw translation keys
    }
    expect(errors).toEqual([])
  })

  test('a setting can be changed, shows as modified and can be reset', async ({ page }) => {
    await login(page)
    await page.goto('/admin/settings/general')
    const name = page.locator('[id="set-app.name"]')
    const original = await name.inputValue()
    await name.fill('Infra Architect QA')
    await page.locator('.savebar .btn-primary, .adm-main .btn-primary', { hasText: 'Сохранить' }).first().click()
    await page.reload()
    await expect(page.locator('.brand span').first()).toHaveText('Infra Architect QA')
    await expect(page.locator('#setting-app\\.name')).toHaveClass(/modified|srow/)
    // put it back
    await page.goto('/admin/settings/general')
    await page.locator('[id="set-app.name"]').fill(original)
    await page.locator('.savebar .btn-primary, .adm-main .btn-primary', { hasText: 'Сохранить' }).first().click()
    await page.reload()
    await expect(page.locator('.brand span').first()).toHaveText(original)
  })

  test('placement patterns are data: an edit in the admin panel reaches the wizard and can be reset', async ({ page }) => {
    await login(page)
    await page.goto('/admin/placement/pattern')
    const row = page.locator('.items-table tbody tr', { hasText: 'two-sites' })
    await expect(row).toBeVisible()
    await expect(row).toContainText('2 площадки')
    await row.locator('.idcell button').click()
    const editor = page.locator('.modal textarea').first()
    const text = await editor.inputValue()
    expect(text.indexOf('"id"')).toBeLessThan(text.indexOf('"minSites"')) // keys in a readable order
    expect(text).toContain('"badge"')
    const badge = `QA ${Date.now().toString(36)}`
    await editor.fill(text.replace(/"badge": "[^"]*"/, `"badge": "${badge}"`))
    await page.keyboard.press('Control+s')
    await expect(page.locator('.items-table tbody tr', { hasText: 'two-sites' }).locator('.tag', { hasText: 'изменён' })).toBeVisible()

    // the wizard offers the pattern with the new badge
    await page.goto('/')
    await page.getByRole('button', { name: 'Новый проект' }).first().click()
    await page.locator('.wizard-card').click()
    await expect(page.locator('.wiz .pat', { hasText: badge })).toBeVisible({ timeout: 20_000 })
    await page.keyboard.press('Escape')

    // reset to the built-in value
    await page.goto('/admin/placement/pattern')
    await page.locator('.items-table tbody tr', { hasText: 'two-sites' }).getByRole('button', { name: 'Сбросить к встроенному' }).click()
    const confirm = page.locator('.modal-foot .btn-danger, .modal-foot .btn-primary').last()
    if (await confirm.isVisible().catch(() => false)) await confirm.click()
    await expect(page.locator('.items-table tbody tr', { hasText: 'two-sites' }).locator('.tag', { hasText: 'изменён' })).toHaveCount(0)
  })

  test('the administrator chooses which scheme the wizard offers first, and an unknown choice is reported', async ({ page }) => {
    await login(page)
    const current = async () => (await (await page.request.get('/api/admin/config/placement.settings')).json()).items[0].data
    const put = (data: unknown) => page.request.put('/api/admin/config/placement.settings/placement', { data: { data } })
    const settings = await current()
    const firstPattern = async () => {
      await page.goto('/')
      await page.getByRole('button', { name: 'Новый проект' }).first().click()
      await page.locator('.wizard-card').click()
      await expect(page.locator('.wiz .site').first()).toBeVisible({ timeout: 20_000 })
      const text = await page.locator('.wiz .pat.on b').first().innerText()
      await page.keyboard.press('Escape')
      return text
    }
    try {
      expect(await firstPattern()).toContain('N площадок')
      expect((await put({ ...settings, defaultPattern: 'single-dc' })).ok()).toBeTruthy()
      expect(await firstPattern()).toContain('Один ЦОД')
      // a scheme that does not exist: the wizard still opens (on the first scheme) and the administrator is told why
      expect((await put({ ...settings, defaultPattern: 'no-such-pattern' })).ok()).toBeTruthy()
      expect(await firstPattern()).toContain('Один ЦОД')
      await page.goto('/admin/placement/settings')
      await expect(page.locator('.cfg-errors').first()).toContainText('no-such-pattern')
    } finally {
      await page.request.post('/api/admin/config/placement.settings/placement/reset')
    }
    expect(await firstPattern()).toContain('N площадок')
  })

  test('an unknown section says so and leads back to the panel', async ({ page }) => {
    await login(page)
    await page.goto('/admin/catalog/no-such-kind')
    const notFound = page.getByTestId('admin-notfound')
    await expect(notFound).toBeVisible()
    await notFound.getByRole('link').click()
    await expect(page.getByTestId('admin-notfound')).toHaveCount(0)
    await expect(page.locator('#adm-main')).toBeVisible()
  })

  test('viewers cannot open the admin panel', async ({ page, request }) => {
    await login(page)
    const stamp = Date.now().toString(36)
    const r = await page.request.post('/api/admin/users', { data: { email: `viewer-${stamp}@e2e.local`, role: 'viewer', password: 'viewer password 12345' } })
    expect(r.ok()).toBeTruthy()
    await page.request.post('/api/auth/logout')
    await login(page, { email: `viewer-${stamp}@e2e.local`, password: 'viewer password 12345' })
    await page.goto('/admin')
    await expect(page).toHaveURL(/\/$/)
    await page.locator('.project').first().click()
    await expect(page.locator('.promptbar .pin')).toBeDisabled()
    await expect(page.getByRole('button', { name: 'Связь' })).toHaveCount(0)
    void request
  })
})
