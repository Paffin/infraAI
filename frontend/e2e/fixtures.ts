import { test as base } from '@playwright/test'

/**
 * The tests create projects through the UI; on a long-lived stack they would pile up until the per-user project limit
 * (`limits.maxProjectsPerUser`) stops the next run. Every test cleans up after itself: whatever `POST /api/projects`
 * created while it ran is deleted when it ends.
 */
export const test = base.extend<{ cleanup: void }>({
  cleanup: [
    async ({ page }, use) => {
      const created: string[] = []
      page.on('response', async (res) => {
        if (res.request().method() !== 'POST' || !res.ok() || !/\/api\/projects$/.test(res.url())) return
        try { created.push((await res.json()).id) } catch { /* not JSON: not ours */ }
      })
      await use()
      for (const id of created) await page.request.delete(`/api/projects/${id}`).catch(() => undefined)
    },
    { auto: true },
  ],
})
