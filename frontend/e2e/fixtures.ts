import { test as base } from '@playwright/test'

/**
 * Every test starts as a user who switched the guided tours off ("do not show again"): a tour is a veil over the whole
 * screen, and the tests are about everything else. `tour.spec.ts` removes the switch. The same user has dismissed the hint that AI is off
 * (`ia_ai_hint_off`; `ai-off.spec.ts` removes that one).
 *
 * The tests create projects through the UI; on a long-lived stack they would pile up until the per-user project limit
 * (`limits.maxProjectsPerUser`) stops the next run. Every test cleans up after itself: whatever `POST /api/projects`
 * created while it ran is deleted when it ends. What the page's own requests created is seen on the wire; what a helper created
 * through the API (`projectWith`) is announced by the helper (`madeByApi`), because requests of the API client are not the page's.
 */
export const madeByApi: string[] = []

export const test = base.extend<{ cleanup: void }>({
  cleanup: [
    async ({ page }, use) => {
      await page.addInitScript(() => {
        try {
          localStorage.setItem('ia_tour_off', '1')
          // the administrator's hint that AI is off (the CI stack has no key) is a strip over the editor: the tests are about the editor, not about the strip
          localStorage.setItem('ia_ai_hint_off', '1')
        } catch { /* no storage: no tour either */ }
      })
      const created: string[] = []
      page.on('response', async (res) => {
        if (res.request().method() !== 'POST' || !res.ok() || !/\/api\/projects$/.test(res.url())) return
        try { created.push((await res.json()).id) } catch { /* not JSON: not ours */ }
      })
      await use()
      for (const id of [...created, ...madeByApi.splice(0)]) await page.request.delete(`/api/projects/${id}`).catch(() => undefined)
    },
    { auto: true },
  ],
})
