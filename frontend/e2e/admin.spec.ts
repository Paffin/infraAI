import { expect, test } from '@playwright/test'
import { login } from './helpers'

test.describe('admin panel', () => {
  test('every section renders without errors', async ({ page }) => {
    const errors: string[] = []
    page.on('pageerror', (e) => errors.push(e.message))
    page.on('console', (m) => { if (m.type() === 'error' && !m.text().includes('401')) errors.push(m.text()) })
    await login(page)
    for (const path of ['settings', 'llm', 'catalog/catalog.kind', 'rules', 'templates', 'prompts', 'engine', 'backup', 'users', 'audit', 'system']) {
      await page.goto('/admin/' + path)
      await expect(page.locator('#adm-main')).toBeVisible()
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
