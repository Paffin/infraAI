import { expect, type Page } from '@playwright/test'
import { test } from './fixtures'
import { login, uniqueName } from './helpers'

// The gallery of reference architectures in the "New project" dialog: a search that looks at names, descriptions and tags, filters by the tags the templates carry,
// cards that say what each promises, and a project that opens with the design that was picked.

interface Tpl { id: string; name: { ru: string; en: string }; tags: string[]; nodeCount: number }

async function templates(page: Page): Promise<Tpl[]> {
  const boot = await (await page.request.get('/api/bootstrap')).json()
  return boot.templates as Tpl[]
}

async function openDialog(page: Page) {
  await page.getByRole('button', { name: 'Новый проект' }).first().click()
  await expect(page.getByTestId('gallery')).toBeVisible()
}

const cards = (page: Page) => page.locator('[data-testid="gallery"] [data-template]')

test.describe('the gallery of reference architectures', () => {
  test.beforeEach(async ({ page }) => {
    await page.addInitScript(() => { try { localStorage.setItem('ia_tour_off', '1') } catch { /* no storage: the tour may open, the test does not need it closed */ } })
    await login(page)
  })

  test('every reference architecture is a card, with what it promises, and the blank sheet comes first', async ({ page }) => {
    const all = await templates(page)
    expect(all.length).toBeGreaterThanOrEqual(20)
    await openDialog(page)
    await expect(cards(page)).toHaveCount(all.length)
    await expect(page.getByTestId('gallery-count')).toHaveText(`Найдено: ${all.length}`)
    // the blank sheet is the first card and is chosen to begin with
    await expect(page.locator('[data-testid="gallery"] .tpl').first()).toHaveAttribute('data-testid', 'tpl-blank')
    await expect(page.getByTestId('tpl-blank')).toHaveClass(/\bon\b/)
    // a card says the size and the goals
    const aws = page.locator('[data-template="aws-three-az"]')
    await expect(aws).toContainText('AWS')
    await expect(aws).toContainText(/узлов/)
    await expect(aws).toContainText('SLO 99.7%')
    await expect(aws).toContainText('RPO 0')
    await expect(aws).toContainText('RTO 5 мин')
  })

  test('the search looks at names, descriptions and tags; a tag button filters by exactly what the architectures carry', async ({ page }) => {
    const all = await templates(page)
    await openDialog(page)

    await page.getByTestId('gallery-search').fill('aws')
    const expected = all.filter((t) => `${t.name.ru} ${t.name.en} ${t.tags.join(' ')} ${t.id}`.toLowerCase().includes('aws')).map((t) => t.id)
    expect(expected).toContain('aws-three-az')
    expect(expected).toContain('serverless-aws')
    await expect(cards(page)).toHaveCount(expected.length)
    await expect(page.locator('[data-template="aws-three-az"]')).toBeVisible()
    await expect(page.locator('[data-template="two-dc-witness"]')).toHaveCount(0)
    await expect(page.getByTestId('gallery-count')).toHaveText(`Найдено: ${expected.length}`)

    // what a tag is called in the interface is searchable too
    await page.getByTestId('gallery-search').fill('свой цод')
    const onPrem = all.filter((t) => t.tags.includes('on-prem')).map((t) => t.id)
    await expect(cards(page)).toHaveCount(onPrem.length)
    await page.getByTestId('gallery-search').fill('')

    // a tag button keeps what carries the tag
    await page.locator('[data-testid="gallery"] [data-tag="kubernetes"]').click()
    const withTag = all.filter((t) => t.tags.includes('kubernetes')).map((t) => t.id)
    expect(withTag.length).toBeGreaterThan(3)
    expect(withTag.length).toBeLessThan(all.length)
    await expect(cards(page)).toHaveCount(withTag.length)
    await expect(page.locator('[data-testid="gallery"] [data-tag="kubernetes"]')).toHaveAttribute('aria-pressed', 'true')
    // and a second click on it, or "All", brings everything back
    await page.locator('[data-testid="gallery"] [data-tag="kubernetes"]').click()
    await expect(cards(page)).toHaveCount(all.length)
    await page.locator('[data-testid="gallery"] [data-tag="ha"]').click()
    await page.getByRole('button', { name: 'Все', exact: true }).click()
    await expect(cards(page)).toHaveCount(all.length)
  })

  test('nothing found says so and leaves the blank sheet to choose', async ({ page }) => {
    await openDialog(page)
    await page.getByTestId('gallery-search').fill('qqqqzzzz')
    await expect(cards(page)).toHaveCount(0)
    await expect(page.getByTestId('gallery-none')).toBeVisible()
    await expect(page.getByTestId('gallery-count')).toHaveText('Найдено: 0')
    await expect(page.getByTestId('tpl-blank')).toBeVisible()
  })

  test('a card that is picked is the design the project opens with', async ({ page }) => {
    await openDialog(page)
    await page.locator('.modal input.input').first().fill(uniqueName('gallery'))
    await page.getByTestId('gallery-search').fill('rag')
    await page.locator('[data-template="rag-llm"]').click()
    await expect(page.locator('[data-template="rag-llm"]')).toHaveClass(/\bon\b/)
    await expect(page.getByTestId('tpl-blank')).not.toHaveClass(/\bon\b/)
    await page.locator('.modal-foot .btn-primary').click()
    await page.waitForSelector('.canvas')
    await expect(page.locator('.arch-node', { hasText: 'vLLM' }).first()).toBeVisible({ timeout: 20_000 })
    // it is a clean design: the engine has nothing to say against it
    await expect(page.getByTestId('all-clear')).toBeVisible({ timeout: 25_000 })
  })

  test('the tags and the counter speak the language of the interface', async ({ page }) => {
    await page.getByRole('button', { name: 'EN' }).click()
    await page.getByRole('button', { name: 'New project' }).first().click()
    await expect(page.getByTestId('gallery')).toBeVisible()
    await expect(page.getByTestId('gallery-count')).toHaveText(/^Found: \d+$/)
    await expect(page.locator('[data-testid="gallery"] [data-tag="on-prem"]')).toHaveText(/On-prem/)
    await expect(page.locator('[data-template="two-dc-witness"]')).toContainText('2 DCs + witness')
    await page.getByTestId('gallery-search').fill('on-prem')
    await expect(page.locator('[data-template="two-dc-witness"]')).toBeVisible()
  })
})
