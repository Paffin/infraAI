import { expect, type Page } from '@playwright/test'
import type { Architecture } from '../src/api/types'
import { madeByApi } from './fixtures'

export const email = process.env.E2E_EMAIL ?? 'admin@example.com'
export const password = process.env.E2E_PASSWORD ?? ''

export async function login(page: Page, as = { email, password }) {
  await page.goto('/login')
  await page.fill('#email', as.email)
  await page.fill('#password', as.password)
  await page.click('button.btn-primary')
  await expect(page).toHaveURL(/\/$/)
}

/** Creates a project through the UI (optionally from a reference architecture) and opens its editor. */
export async function createProject(page: Page, name: string, template?: string) {
  await page.getByRole('button', { name: 'Новый проект' }).first().click()
  await page.fill('.modal input.input', name)
  if (template) await page.locator('.tpl', { hasText: template }).click()
  await page.locator('.modal-foot .btn-primary').click()
  await page.waitForSelector('.canvas')
}

export const uniqueName = (prefix: string) => `${prefix}-${Date.now().toString(36)}`

/** Opens one of the analyses (Ёмкость, Доступность, Стоимость, Площадки): the Анализ tab of the right panel, on that view. */
export async function openAnalysis(page: Page, view: 'Ёмкость' | 'Доступность' | 'Стоимость' | 'Площадки') {
  await page.locator('.right .tab', { hasText: 'Анализ' }).click()
  await page.locator('.right .seg .segbtn', { hasText: view }).click()
}

/** A project whose working copy is `arch` (made through the API, so a test can start from any design); returns its id. */
export async function projectWith(page: Page, name: string, arch: Architecture): Promise<string> {
  const created = await page.request.post('/api/projects', { data: { name } })
  expect(created.ok()).toBe(true)
  const id = (await created.json()).id as string
  madeByApi.push(id)
  const cur = await (await page.request.get(`/api/projects/${id}`)).json()
  const put = await page.request.put(`/api/projects/${id}/draft`, { data: { architecture: arch, expectedRev: cur.rev } })
  expect(put.ok(), await put.text()).toBe(true)
  return id
}
