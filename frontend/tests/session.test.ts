import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import type { Architecture } from '../src/api/types'
import { onUnauthorized } from '../src/lib/session'
import { keyOf, PREFIX } from '../src/lib/unsavedBackup'
import { useApp } from '../src/stores/app'
import { useEditor } from '../src/stores/editor'
import { fixture } from './fixtures'

// The session ends under an open editor. Real pinia, the real store of the editor, a real router (in memory) and the real storage of jsdom: the page that is left
// is the page of a person who was typing.

const arch: Architecture = fixture('report-spof').architecture
const opened: ReturnType<typeof useEditor>[] = []

function makeRouter(): Router {
  const page = { render: () => null }
  return createRouter({ history: createMemoryHistory(), routes: [{ path: '/login', name: 'login', component: page }, { path: '/p/:id', name: 'editor', component: page }, { path: '/', name: 'projects', component: page }] })
}

/** A person who has a project open (as `load` leaves it: the server's draft on the screen, at revision 4). */
async function editing(role = 'editor') {
  const app = useApp()
  app.user = { id: 'u1', email: 'a@b.c', name: 'A', role, mustChangePassword: false } as never
  app.boot = { editor: {} } as never
  const editor = useEditor()
  opened.push(editor)
  editor.projectId = 'p1'
  editor.arch = arch
  editor.rev = 4
  const router = makeRouter()
  await router.push('/p/p1')
  return { app, editor, router }
}

beforeEach(() => {
  setActivePinia(createPinia())
  localStorage.clear()
})

afterEach(() => {
  for (const e of opened.splice(0)) e.reset()
  localStorage.clear()
})

describe('a 401 under an open editor', () => {
  it('keeps the edit that never reached the server, and says so on the sign-in page', async () => {
    const { app, editor, router } = await editing()
    editor.setMeta('Edited before the session ended')
    expect(editor.saveState).toBe('dirty') // the autosave has not run: the edit is on the screen only

    onUnauthorized(router)
    await new Promise((r) => setTimeout(r, 0))

    expect(app.user).toBeNull()
    expect(router.currentRoute.value.name).toBe('login')
    expect(router.currentRoute.value.query).toEqual({ next: '/p/p1', expired: 'kept' })
    // the copy is under the key of the user who typed it, although the user was forgotten right after: the order is what makes that so
    const copy = JSON.parse(localStorage.getItem(keyOf('u1', 'p1'))!)
    expect(copy).toMatchObject({ v: 1, userId: 'u1', projectId: 'p1', baseRev: 4 })
    expect(copy.architecture.meta.name).toBe('Edited before the session ended')
    expect(copy.architecture.nodes).toEqual(arch.nodes)
  })

  it('a save in flight, a failed save and a conflict are unsaved too', async () => {
    for (const how of ['saving', 'error'] as const) {
      localStorage.clear()
      setActivePinia(createPinia())
      const { editor, router } = await editing()
      editor.setMeta(how)
      editor.projectId = 'p1'
      void editor.flushSave() // a request goes out (there is no server: it fails, as it does with the connection gone)
      if (how === 'error') await editor.flushSave()
      expect(editor.saveState === 'saving' || editor.saveState === 'error').toBe(true)
      onUnauthorized(router)
      await new Promise((r) => setTimeout(r, 0))
      expect(router.currentRoute.value.query.expired, how).toBe('kept')
      expect(JSON.parse(localStorage.getItem(keyOf('u1', 'p1'))!).architecture.meta.name, how).toBe(how)
    }
  })

  it('has nothing to keep when everything is saved: the page only says the session ended', async () => {
    const { router } = await editing()
    onUnauthorized(router)
    await new Promise((r) => setTimeout(r, 0))
    expect(router.currentRoute.value.query).toEqual({ next: '/p/p1', expired: '1' })
    expect(localStorage.length).toBe(0)
  })

  it('does not claim to have kept the edit when the browser would not keep it', async () => {
    const { editor, router } = await editing()
    editor.setMeta('Lost')
    let n = 0
    let size = 1024 * 1024
    while (size >= 16) {
      try { localStorage.setItem(`fill${n++}`, 'x'.repeat(size)) } catch { size = Math.floor(size / 4) }
    }
    onUnauthorized(router)
    await new Promise((r) => setTimeout(r, 0))
    expect(router.currentRoute.value.query.expired).toBe('lost')
    expect(Object.keys(localStorage).some((k) => k.startsWith(PREFIX))).toBe(false)
  })

  it('on a page that is not the editor there is nothing to keep', async () => {
    const app = useApp()
    app.user = { id: 'u1', email: 'a@b.c', name: 'A', role: 'editor', mustChangePassword: false } as never
    app.boot = { editor: {} } as never
    const router = makeRouter()
    await router.push('/')
    onUnauthorized(router)
    await new Promise((r) => setTimeout(r, 0))
    expect(router.currentRoute.value.query).toEqual({ next: '/', expired: '1' })
  })

  it('is answered once: the other requests that were in flight answer 401 too', async () => {
    const { editor, router } = await editing()
    editor.setMeta('Once')
    onUnauthorized(router)
    await new Promise((r) => setTimeout(r, 0))
    const first = router.currentRoute.value.fullPath
    onUnauthorized(router)
    onUnauthorized(router)
    await new Promise((r) => setTimeout(r, 0))
    expect(router.currentRoute.value.fullPath).toBe(first)
    expect(Object.keys(localStorage).filter((k) => k.startsWith(PREFIX))).toEqual([keyOf('u1', 'p1')])
  })
})
