import type { Router } from 'vue-router'
import { useApp } from '@/stores/app'
import { useUnsaved } from '@/stores/unsaved'

/**
 * The server answered 401: the session is over (it ran out, or the account was disabled or its sessions revoked). The person goes to the sign-in page and is told
 * why, and what they typed before the session ended is not lost with it.
 *
 * The ORDER is the point. Once the user is forgotten the editor cannot edit (`canEdit` is false), cannot save, and no longer knows whose edits they were; so the
 * copy of the unsaved edits is made first, while all of that is still known. The first 401 is usually not the save at all but the check that runs a third of a
 * second after an edit, long before the autosave: the edit is still only on the screen.
 */
export function onUnauthorized(router: Pick<Router, 'push' | 'currentRoute'>): void {
  const app = useApp()
  if (!app.user) return // already signed out: the other requests that were in flight answer 401 too
  const kept = useUnsaved().onSessionExpired()
  app.user = null
  void router.push({ name: 'login', query: { next: router.currentRoute.value.fullPath, expired: kept === 'none' ? '1' : kept } })
}
