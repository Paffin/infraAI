import { expect, type Page } from '@playwright/test'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { test } from './fixtures'
import { login, uniqueName } from './helpers'
import type { Architecture } from '../src/api/types'

/** A recorded run of the chat: "make it fault tolerant within one data center" (tests/fixtures/ai-one-dc-*.json). */
const recorded = (name: string) => JSON.parse(readFileSync(join(process.cwd(), 'tests', 'fixtures', name), 'utf8')) as { previous: Architecture; next: Architecture }

async function projectWith(page: Page, name: string, arch: Architecture): Promise<string> {
  const created = await page.request.post('/api/projects', { data: { name } })
  expect(created.ok()).toBe(true)
  const id = (await created.json()).id as string
  const cur = await (await page.request.get(`/api/projects/${id}`)).json()
  const put = await page.request.put(`/api/projects/${id}/draft`, { data: { architecture: arch, expectedRev: cur.rev } })
  expect(put.ok(), await put.text()).toBe(true)
  return id
}

/** Is the card drawn inside the zone it belongs to (the centre of the card decides, like when it is dropped)? */
async function cardInZone(page: Page, nodeId: string, zoneId: string): Promise<boolean> {
  return page.evaluate(([n, z]) => {
    const card = document.querySelector(`[data-id="n:${n}"]`)?.getBoundingClientRect()
    const zone = document.querySelector(`[data-id="z:${z}"]`)?.getBoundingClientRect()
    if (!card || !zone) return false
    const cx = card.left + card.width / 2
    const cy = card.top + card.height / 2
    return cx >= zone.left && cx <= zone.right && cy >= zone.top && cy <= zone.bottom
  }, [nodeId, zoneId])
}

/** Is the zone drawn inside its parent? */
async function zoneInZone(page: Page, zoneId: string, parentId: string): Promise<boolean> {
  return page.evaluate(([z, p]) => {
    const a = document.querySelector(`[data-id="z:${z}"]`)?.getBoundingClientRect()
    const b = document.querySelector(`[data-id="z:${p}"]`)?.getBoundingClientRect()
    return !!a && !!b && a.left >= b.left - 2 && a.top >= b.top - 2 && a.right <= b.right + 2 && a.bottom <= b.bottom + 2
  }, [zoneId, parentId])
}

test.describe('zones that were rearranged are drawn as the model says', () => {
  test.beforeEach(async ({ page }) => {
    await login(page)
  })

  test('a project left with its cards outside their racks offers to lay the diagram out again, and does', async ({ page }) => {
    // what a user got after the chat moved the arbiter and the backups to an extra site: racks beside the data center, empty,
    // their cards still in the old box
    const { next } = recorded('ai-one-dc-extra-site.json')
    const id = await projectWith(page, uniqueName('drift'), next)
    await page.goto(`/p/${id}`)
    await expect(page.locator('.arch-node').first()).toBeVisible()
    const note = page.getByTestId('drift-note')
    await expect(note).toBeVisible()
    expect(await cardInZone(page, 'app', 'dc1-r1')).toBe(false)

    await note.getByRole('button', { name: 'Перестроить схему' }).click()
    await expect(note).toHaveCount(0)
    // every card is in the rack it belongs to, every rack in the data center
    for (const n of next.nodes.filter((x) => x.zoneId)) expect(await cardInZone(page, n.id, n.zoneId!), `${n.id} in ${n.zoneId}`).toBe(true)
    for (const z of next.zones.filter((x) => x.parentId)) expect(await zoneInZone(page, z.id, z.parentId!), `${z.id} in ${z.parentId}`).toBe(true)

    // the layout is what is stored: after a reload there is nothing to offer
    await expect.poll(async () => (await (await page.request.get(`/api/projects/${id}`)).json()).architecture.zones.every((z: { ui?: unknown }) => !!z.ui)).toBe(true)
    await page.waitForTimeout(1200)
    await page.reload()
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await expect(page.getByTestId('drift-note')).toHaveCount(0)
  })

  test('the notice can be dismissed and a coherent diagram never shows it', async ({ page }) => {
    const { next, previous } = recorded('ai-one-dc-racks.json')
    const id = await projectWith(page, uniqueName('drift-dismiss'), next)
    await page.goto(`/p/${id}`)
    await expect(page.getByTestId('drift-note')).toBeVisible()
    await page.getByTestId('drift-note').getByRole('button', { name: 'Закрыть' }).click()
    await expect(page.getByTestId('drift-note')).toHaveCount(0)

    // the scheme the user drew by hand is coherent
    const calm = await projectWith(page, uniqueName('drift-calm'), previous)
    await page.goto(`/p/${calm}`)
    await expect(page.locator('.arch-node').first()).toBeVisible()
    await page.waitForTimeout(800)
    await expect(page.getByTestId('drift-note')).toHaveCount(0)
  })

  test('a zone put into another one in the inspector is drawn inside it, with its cards', async ({ page }) => {
    const arch: Architecture = {
      schemaVersion: 1, meta: {}, requirements: {},
      zones: [
        { id: 'dc', name: 'Data center', type: 'dc', parentId: null, role: null, props: {}, ui: { x: 0, y: 0, w: 420, h: 360 } },
        { id: 'rack', name: 'Rack A', type: 'rack', parentId: null, role: null, props: {}, ui: { x: 600, y: 0, w: 320, h: 300 } },
      ],
      nodes: [
        { id: 'app', kind: 'kubernetes', name: 'App', zoneId: 'dc', group: null, role: null, props: {}, ui: { x: 40, y: 70 } },
        { id: 'db', kind: 'postgresql', name: 'Database', zoneId: 'rack', group: null, role: null, props: {}, ui: { x: 640, y: 70 } },
      ],
      edges: [{ id: 'e1', source: 'app', target: 'db', type: 'traffic', label: null, props: {} }],
    }
    const id = await projectWith(page, uniqueName('nest'), arch)
    await page.goto(`/p/${id}`)
    await expect(page.locator('.arch-node').first()).toBeVisible()
    expect(await zoneInZone(page, 'rack', 'dc')).toBe(false)
    await page.locator('[data-id="z:rack"] .zone-head').click()
    await page.locator('.inspector label.prop', { hasText: 'Вложена в' }).locator('select').selectOption({ label: 'Data center' })
    await expect.poll(() => zoneInZone(page, 'rack', 'dc'), { timeout: 10_000 }).toBe(true)
    expect(await cardInZone(page, 'db', 'rack')).toBe(true)
    expect(await cardInZone(page, 'app', 'dc')).toBe(true)
  })
})
