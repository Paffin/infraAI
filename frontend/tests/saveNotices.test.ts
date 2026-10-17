import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { nextTick } from 'vue'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import type { Architecture } from '../src/api/types'
import SaveStatusBar from '../src/editor/SaveStatusBar.vue'
import { setLang, t } from '../src/i18n'
import { keyOf, PREFIX, write } from '../src/lib/unsavedBackup'
import LoginView from '../src/views/LoginView.vue'
import { useApp } from '../src/stores/app'
import { useEditor } from '../src/stores/editor'
import { useUnsaved } from '../src/stores/unsaved'
import { fixture, realIndex } from './fixtures'

// The two places the page says what happened to the edits, mounted for real with real stores and a real router: the strip of the editor and the sign-in page.

const arch: Architecture = fixture('report-spof').architecture
const mounted: VueWrapper[] = []
const opened: ReturnType<typeof useEditor>[] = []

function router(): Router {
  const page = { render: () => null }
  return createRouter({ history: createMemoryHistory(), routes: [{ path: '/login', name: 'login', component: LoginView }, { path: '/p/:id', name: 'editor', component: page }, { path: '/', name: 'projects', component: page }] })
}

function editorWithProject() {
  const app = useApp()
  app.user = { id: 'u1', email: 'a@b.c', name: 'A', role: 'editor', mustChangePassword: false } as never
  app.boot = { editor: { saveRetryMs: 60000 } } as never
  app.index = realIndex()
  const editor = useEditor()
  opened.push(editor)
  editor.projectId = 'p1'
  editor.arch = arch
  editor.rev = 4
  return { app, editor, unsaved: useUnsaved() }
}

async function strip() {
  const r = router()
  await r.push('/p/p1')
  const wrapper = mount(SaveStatusBar, { global: { plugins: [r] }, attachTo: document.body })
  mounted.push(wrapper)
  return wrapper
}

beforeEach(() => {
  setActivePinia(createPinia())
  localStorage.clear()
  setLang('ru')
})

afterEach(() => {
  for (const m of mounted.splice(0)) m.unmount()
  for (const e of opened.splice(0)) e.reset()
  document.body.innerHTML = ''
  localStorage.clear()
})

describe('the strip of the editor', () => {
  it('says nothing while all goes well, and takes no room', async () => {
    editorWithProject()
    const w = await strip()
    expect(w.find('.sb').exists()).toBe(false)
    expect(w.find('[data-testid="save-live"]').attributes('role')).toBe('status') // the live region is there before there is anything to say
    expect(w.text()).toBe('')
  })

  it('says there is no connection in the words of the page, and again in the other language without a new failure', async () => {
    const { editor } = editorWithProject()
    const w = await strip()
    editor.setMeta('typed')
    await editor.flushSave() // no server at the address of the page: the request fails as it does with no network
    await nextTick()
    const notice = w.find('[data-failure="offline"]')
    expect(notice.exists()).toBe(true)
    expect(notice.text()).toContain(t('editor.save.offline'))
    expect(notice.text()).toContain(t('editor.save.retry'))
    expect(w.find('[data-testid="save-live"]').element.contains(notice.element)).toBe(true) // inside the live region, so it is read out when it appears
    expect(w.text()).not.toMatch(/fetch|parse|URL/i)
    setLang('en')
    await nextTick()
    expect(w.find('[data-failure="offline"]').text()).toContain('Cannot reach the server')
  })

  it('"Retry now" sends the document again', async () => {
    const { editor } = editorWithProject()
    const w = await strip()
    editor.setMeta('typed')
    await editor.flushSave()
    await nextTick()
    expect(editor.saveState).toBe('error')
    // the click starts the request in the same breath (the state is `saving` before anything has been awaited); with no server it fails again
    ;(w.find('[data-failure="offline"] button').element as HTMLButtonElement).click()
    expect(editor.saveState).toBe('saving')
    await flushPromises()
    expect(editor.saveState).toBe('error')
  })

  it('has nothing to say about a document that is waiting to be saved and has not failed to be', async () => {
    const { editor } = editorWithProject()
    editor.setMeta('typed') // dirty, not failing
    const w = await strip()
    expect(w.find('.sb').exists()).toBe(false)
  })

  it('offers the edits found in this browser, with what they change and the three answers', async () => {
    const { editor, unsaved } = editorWithProject()
    const edited: Architecture = { ...arch, nodes: arch.nodes.map((n, i) => (i === 0 ? { ...n, name: 'Typed name' } : n)) }
    write(localStorage, { userId: 'u1', projectId: 'p1', baseRev: 4, savedAt: Date.now(), architecture: edited }, { now: Date.now(), keepMs: 1e9 })
    const w = await strip()
    await unsaved.check('p1')
    await nextTick()
    const offer = w.find('[data-testid="unsaved-offer"]')
    expect(offer.exists()).toBe(true)
    expect(offer.text()).toContain(t('unsaved.text'))
    expect(offer.find('[data-testid="unsaved-restore"]').exists()).toBe(true)
    expect(offer.find('[data-testid="unsaved-discard"]').exists()).toBe(true)
    expect(offer.find('.sb-moved').exists()).toBe(false) // the revision is the one the edits were made on
    expect(offer.find('[data-testid="unsaved-new"]').exists()).toBe(false) // nothing to protect: no third answer
    expect(editor.rev).toBe(4)

    editor.rev = 9 // the server moved on
    await unsaved.check('p1')
    await nextTick()
    expect(w.find('.sb-moved').text()).toBe(t('unsaved.moved'))
    expect(w.find('[data-testid="unsaved-new"]').exists()).toBe(true)
    expect(Object.keys(localStorage).filter((k) => k.startsWith(PREFIX))).toEqual([keyOf('u1', 'p1')])
  })
})

describe('the sign-in page after the session ended', () => {
  async function loginPage(query: string) {
    const r = router()
    await r.push('/login' + query)
    const w = mount(LoginView, { global: { plugins: [r] }, attachTo: document.body })
    mounted.push(w)
    await flushPromises()
    return w
  }

  it('says that the session ended and that the edits are kept, when they are', async () => {
    const w = await loginPage('?next=/p/p1&expired=kept')
    const said = w.get('[data-testid="login-expired"]')
    expect(said.attributes('role')).toBe('status')
    expect(said.text()).toContain(t('login.expired'))
    expect(said.text()).toContain(t('login.expired.kept'))
    expect(said.text()).not.toContain(t('login.expired.lost'))
  })

  it('says that they could not be kept, when they could not', async () => {
    const w = await loginPage('?expired=lost')
    expect(w.get('[data-testid="login-expired"]').text()).toContain(t('login.expired.lost'))
    expect(w.get('[data-testid="login-expired"]').text()).not.toContain(t('login.expired.kept'))
  })

  it('says only that the session ended when nothing was unsaved: nothing is promised', async () => {
    const w = await loginPage('?expired=1')
    const text = w.get('[data-testid="login-expired"]').text()
    expect(text).toBe(t('login.expired'))
    expect(text).not.toContain('браузере')
  })

  it('says nothing on an ordinary visit, or for a value nobody sends', async () => {
    expect((await loginPage('')).find('[data-testid="login-expired"]').exists()).toBe(false)
    expect((await loginPage('?expired=whatever')).find('[data-testid="login-expired"]').exists()).toBe(false)
  })

  it('speaks the language of the page', async () => {
    setLang('en')
    const w = await loginPage('?expired=kept')
    expect(w.get('[data-testid="login-expired"]').text()).toContain('Your session has ended')
    expect(w.get('[data-testid="login-expired"]').text()).toContain('kept in this browser')
  })
})
