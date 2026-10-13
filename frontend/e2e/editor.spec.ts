import { expect } from '@playwright/test'
import { test } from './fixtures'
import { createProject, login, uniqueName } from './helpers'

test.describe('editor', () => {
  test.beforeEach(async ({ page }) => {
    await login(page)
  })

  test('a reference architecture is laid out, scored and simulated', async ({ page }) => {
    await createProject(page, uniqueName('tpl'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await expect(page.locator('.arch-zone')).toHaveCount(3)
    await expect(page.locator('.score b')).toHaveText('100')
    await expect(page.locator('.tile.ok b')).not.toHaveText('0')

    await page.locator('.whatif .w', { hasText: 'ЦОД-1' }).first().click()
    await expect(page.locator('.whatif .res')).toContainText('RPO')
    await expect(page.locator('.simcard')).toBeVisible()
    await page.locator('.whatif .x').click()
    await expect(page.locator('.simcard')).toHaveCount(0)
  })

  test('manual editing: add, connect, rename, delete, undo, persist', async ({ page }) => {
    await createProject(page, uniqueName('manual'))
    await expect(page.locator('.empty-state')).toBeVisible()
    for (const kind of ['Kubernetes', 'PostgreSQL', 'Redis']) {
      await page.locator('.library .kind', { hasText: kind }).first().click()
      await page.waitForTimeout(200)
    }
    await expect(page.locator('.arch-node')).toHaveCount(3)

    // nodes are added at the same spot; spread them so that clicks hit the intended card
    for (let i = 0; i < 3; i++) {
      const box = await page.locator('.arch-node').nth(i).boundingBox()
      await page.mouse.move(box!.x + 60, box!.y + 30)
      await page.mouse.down()
      await page.mouse.move(box!.x + 60 + i * 260 - 200, box!.y + 30 + i * 30, { steps: 6 })
      await page.mouse.up()
    }

    await page.getByRole('button', { name: 'Связь' }).click()
    await page.locator('.arch-node').nth(0).click()
    await page.locator('.arch-node').nth(1).click()
    await expect(page.locator('.vue-flow__edge')).toHaveCount(1)
    await page.getByRole('button', { name: 'Выбор' }).click()

    await page.locator('.arch-node').nth(1).click()
    await page.locator('.inspector .prop input').first().fill('Основная БД')
    await expect(page.locator('.arch-node', { hasText: 'Основная БД' })).toHaveCount(1)

    await page.locator('.arch-node').nth(2).click()
    await page.keyboard.press('Delete')
    await expect(page.locator('.arch-node')).toHaveCount(2)
    await page.keyboard.press('Control+z')
    await expect(page.locator('.arch-node')).toHaveCount(3)

    await expect(page.locator('.crumbs .meta')).toContainText('сохранено', { timeout: 10_000 })
    await page.reload()
    await expect(page.locator('.arch-node')).toHaveCount(3)
    await expect(page.locator('.arch-node', { hasText: 'Основная БД' })).toHaveCount(1)
  })

  test('findings come with fixes; a risk can be accepted with a reason', async ({ page }) => {
    await createProject(page, uniqueName('findings'), '2 ЦОД + witness')
    await page.locator('.arch-node', { hasText: 'PostgreSQL replica' }).first().click()
    await page.keyboard.press('Delete')

    const card = page.locator('.finding', { has: page.locator('.btn-primary') }).first()
    await expect(card).toBeVisible({ timeout: 15_000 })
    const rule = (await card.locator('.rid').innerText()).trim()
    await card.locator('.btn-primary').first().click()
    await expect(page.locator('.finding .rid', { hasText: rule })).toHaveCount(0, { timeout: 15_000 })

    const accept = page.locator('.finding .btn', { hasText: 'Принять риск' }).first()
    if (await accept.count()) {
      await accept.click()
      await page.fill('.modal textarea', 'Принимаем риск до миграции, согласовано с владельцем сервиса.')
      await page.locator('.modal-foot .btn-primary').click()
      await expect(page.locator('.fold', { hasText: 'Принятые риски' })).toBeVisible()
    }
  })

  test('versions, diff and a conflicting edit from another tab', async ({ page, context }) => {
    await createProject(page, uniqueName('versions'), '2 ЦОД + witness')
    const url = page.url()

    await page.getByRole('button', { name: 'Версии · diff' }).click()
    await page.fill('.modal input.input', 'снимок')
    await page.locator('.modal .btn-primary').first().click()
    await expect(page.locator('.vers .list li')).toHaveCount(2)
    await page.keyboard.press('Escape')

    const other = await context.newPage()
    await other.goto(url)
    await other.locator('.arch-node').first().click()
    await other.locator('.inspector .prop input').first().fill('Из второй вкладки')
    await expect(other.locator('.crumbs .meta')).toContainText('сохранено', { timeout: 10_000 })

    await page.locator('.arch-node').nth(2).click()
    await page.locator('.inspector .prop input').first().fill('Конфликт')
    await expect(page.locator('.conflict')).toBeVisible({ timeout: 10_000 })
    await page.getByRole('button', { name: 'Загрузить версию сервера' }).click()
    await expect(page.locator('.conflict')).toHaveCount(0)
    await expect(page.locator('.arch-node', { hasText: 'Из второй вкладки' })).toHaveCount(1)
  })

  test('requirements drive the objectives', async ({ page }) => {
    await createProject(page, uniqueName('req'), '2 ЦОД + witness')
    await page.locator('.tab', { hasText: 'Требования' }).click()
    const rto = page.locator('.reqs .field', { hasText: 'RTO' })
    await rto.locator('select').selectOption('seconds')
    await rto.locator('input').fill('5')
    await page.locator('.tab', { hasText: 'Проверки' }).click()
    await expect(page.locator('.obj.critical').first()).toBeVisible({ timeout: 15_000 })
  })

  test('drift: an imported snapshot is compared with the design and shown on the canvas', async ({ page }) => {
    await createProject(page, uniqueName('drift'), '2 ЦОД + witness')
    await expect(page.locator('.arch-node').first()).toBeVisible()
    const id = page.url().split('/p/')[1]
    const project = await page.evaluate(async (pid) => (await fetch('/api/projects/' + pid)).json(), id)
    const actual = JSON.parse(JSON.stringify(project.architecture))
    actual.nodes = actual.nodes.filter((n: { id: string }) => n.id !== 'mon-dc1')
    actual.edges = actual.edges.filter((e: { source: string; target: string }) => e.source !== 'mon-dc1' && e.target !== 'mon-dc1')
    actual.nodes.find((n: { id: string }) => n.id === 'app-dc1').props.replicas = 99
    actual.nodes.push({ id: 'shadow-it', kind: 'redis', name: 'Shadow Redis', zoneId: 'dc1', group: null, role: null, props: {} })

    await page.getByRole('button', { name: 'Версии · diff' }).click()
    await page.locator('.drift input[type=file]').setInputFiles({ name: 'actual.json', mimeType: 'application/json', buffer: Buffer.from(JSON.stringify(actual)) })
    await expect(page.locator('.difflist')).toContainText('Shadow Redis')
    await expect(page.locator('.difflist')).toContainText('props.replicas')
    await page.getByRole('button', { name: 'Показать на схеме' }).click()
    await expect(page.locator('.previewbar .exit')).toBeVisible()
    await expect(page.locator('.arch-node.proposed')).not.toHaveCount(0)
    await expect(page.locator('.arch-node.ghost')).not.toHaveCount(0)
    await page.locator('.previewbar .exit').click()
    await expect(page.locator('.previewbar')).toHaveCount(0)
  })
})
