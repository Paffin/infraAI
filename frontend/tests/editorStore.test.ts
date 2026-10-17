import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import type { ArchDiff } from '../src/api/types'
import { setLang, t } from '../src/i18n'
import { useApp } from '../src/stores/app'
import { useEditor } from '../src/stores/editor'

// The real store on real pinia. The one thing a unit test cannot reach is a server, and none is invented here: the page has no server at its address, so the
// request of a save fails the way it fails with no network (`ApiError(0, 'network')`), which is the path these tests need. The attempts are counted by
// wrapping the real `fetch`, which still does what it does.

let attempts = 0
const realFetch = globalThis.fetch
/** Every editor a test opened: each has timers of its own (autosave, retry), and none may outlive its test and make a request in the next one. */
const opened: ReturnType<typeof useEditor>[] = []

beforeEach(() => {
  setActivePinia(createPinia())
  attempts = 0
  globalThis.fetch = ((...a: Parameters<typeof fetch>) => { attempts++; return realFetch(...a) }) as typeof fetch
  setLang('ru')
})

afterEach(() => {
  for (const e of opened.splice(0)) e.reset()
  globalThis.fetch = realFetch
})

/** An editor who has a project open, with the editor timings the test needs (the rest is the fallback of `lib/uiConfig.ts`). */
function open(timings: Record<string, number> = {}) {
  const app = useApp()
  app.user = { id: 'u1', email: 'a@b.c', name: 'A', role: 'editor', mustChangePassword: false }
  app.boot = { editor: timings } as never
  const editor = useEditor()
  opened.push(editor)
  editor.projectId = 'p1'
  return { app, editor }
}

const noDiff = (): ArchDiff => ({ requirements: [], zones: { added: [], removed: [], changed: [] }, nodes: { added: [], removed: [], changed: [] }, edges: { added: [], removed: [], changed: [] } })
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))

describe('undo and redo', () => {
  it('walk back through the edits and forward again', () => {
    const { editor } = open()
    editor.setMeta('one')
    editor.edit((a) => { a.meta = { ...a.meta, name: 'two' } }, { coalesce: 'other' })
    expect(editor.arch.meta.name).toBe('two')
    editor.undo()
    expect(editor.arch.meta.name).toBe('one')
    editor.undo()
    expect(editor.arch.meta.name).toBeUndefined()
    expect(editor.canUndo).toBe(false)
    editor.redo()
    editor.redo()
    expect(editor.arch.meta.name).toBe('two')
    expect(editor.canRedo).toBe(false)
  })

  it('a view that cannot be edited takes no step: the steps are still there when it is closed (they were thrown away before)', () => {
    const { editor } = open()
    editor.edit((a) => { a.meta = { ...a.meta, name: 'one' } }, { coalesce: 'a' })
    editor.edit((a) => { a.meta = { ...a.meta, name: 'two' } }, { coalesce: 'b' })
    expect(editor.canUndo).toBe(true)

    editor.setPreview({ kind: 'compare', label: 'v1', proposalId: '', base: editor.arch, arch: editor.arch, diff: noDiff() })
    expect(editor.readonly).toBe(true)
    editor.undo()
    editor.undo()
    editor.redo()
    expect(editor.arch.meta.name).toBe('two')

    editor.setPreview(null)
    expect(editor.canUndo).toBe(true)
    editor.undo()
    editor.undo()
    expect(editor.arch.meta.name).toBeUndefined()
    expect(editor.canUndo).toBe(false)
  })

  it('a redo step is not consumed by a read-only view either', () => {
    const { editor } = open()
    editor.edit((a) => { a.meta = { ...a.meta, name: 'one' } }, { coalesce: 'a' })
    editor.undo()
    expect(editor.canRedo).toBe(true)
    editor.setPreview({ kind: 'compare', label: 'v1', proposalId: '', base: editor.arch, arch: editor.arch, diff: noDiff() })
    editor.redo()
    editor.setPreview(null)
    expect(editor.canRedo).toBe(true)
    editor.redo()
    expect(editor.arch.meta.name).toBe('one')
  })

  it('merge repeated edits of one thing by the window of the settings (`editor.coalesceMs`)', () => {
    for (const [coalesceMs, undos] of [[600000, 1], [0, 3]] as const) {
      setActivePinia(createPinia())
      const { editor } = open({ coalesceMs })
      editor.setMeta('a')
      editor.setMeta('ab')
      editor.setMeta('abc')
      let n = 0
      while (editor.canUndo) { editor.undo(); n++ }
      expect(n, `window ${coalesceMs} ms`).toBe(undos)
      expect(editor.arch.meta.name).toBeUndefined()
    }
  })

  it('keep as many steps as the settings say (`editor.historySteps`)', () => {
    const { editor } = open({ historySteps: 3 })
    for (let i = 1; i <= 6; i++) editor.edit((a) => { a.meta = { ...a.meta, name: `n${i}` } }, { coalesce: `k${i}` })
    let n = 0
    while (editor.canUndo) { editor.undo(); n++ }
    expect(n).toBe(3)
    expect(editor.arch.meta.name).toBe('n3')
  })
})

describe('the save path in the store', () => {
  it('an edit is dirty, and a save that cannot reach the server is an error that says "offline" in the words of the page, not the browser\'s', async () => {
    const { editor } = open({ saveRetryMs: 60000 })
    editor.setMeta('x')
    expect(editor.saveState).toBe('dirty')
    expect(await editor.flushSave()).toBe(false)
    expect(editor.saveState).toBe('error')
    expect(editor.saveFailure).toBe('offline')
    expect(editor.saveError).toBe(t('error.network'))
    expect(editor.saveError).not.toMatch(/fetch|parse|URL/i)
    setLang('en')
    expect(editor.saveError).toBe(t('error.network')) // and it follows the language
    expect(editor.saveError).toBe('Cannot reach the server')
  })

  it('goes on trying by itself, with the pause of the settings', async () => {
    const { editor } = open({ saveRetryMs: 40, saveRetryMaxMs: 40 })
    editor.setMeta('x')
    await editor.flushSave()
    expect(attempts).toBe(1)
    await sleep(150)
    expect(attempts).toBeGreaterThan(1) // the retry timer fired on its own
  })

  it('"online" does not wait for the pause', async () => {
    const { editor } = open({ saveRetryMs: 60000 })
    editor.setMeta('y')
    await editor.flushSave()
    expect(attempts).toBe(1)
    editor.retryNow() // the network is back
    await sleep(30)
    expect(attempts).toBe(2) // at once, with the pause still 60 s
  })

  it('an edit made while the server cannot be reached is not lost: it is dirty again, the failure is remembered, and the next attempt carries it', async () => {
    const { editor } = open({ saveRetryMs: 60000, autosaveMs: 10 })
    editor.setMeta('first')
    await editor.flushSave()
    expect(editor.saveState).toBe('error')
    editor.setMeta('second')
    expect(editor.saveState).toBe('dirty')
    expect(editor.saveFailure).toBe('offline') // the message does not flicker away with the edit
    const before = attempts
    await sleep(80)
    expect(attempts).toBe(before + 1) // the autosave tried again
    expect(editor.arch.meta.name).toBe('second')
  })

  it('reset() leaves nothing of the project that was open: no pause, no failure, no timer', async () => {
    const { editor } = open({ saveRetryMs: 30, saveRetryMaxMs: 30 })
    editor.setMeta('x')
    await editor.flushSave()
    expect(editor.saveState).toBe('error')
    const before = attempts
    editor.reset()
    expect(editor.saveState).toBe('saved')
    expect(editor.saveFailure).toBeNull()
    expect(editor.saveError).toBe('')
    expect(editor.canUndo).toBe(false)
    await sleep(120)
    expect(attempts).toBe(before) // the retry timer of the old project did not fire
  })

  it('the answer of a project that was left does not change the one that is open', async () => {
    const { editor } = open({ saveRetryMs: 60000 })
    editor.setMeta('x')
    const late = editor.flushSave() // the request of project p1 is out ...
    editor.reset() // ... and the person opens another project before it answers
    editor.projectId = 'p2'
    expect(await late).toBe(false)
    expect(editor.saveState).toBe('saved')
    expect(editor.saveFailure).toBeNull()
    expect(editor.saveError).toBe('')
    await sleep(30)
    expect(editor.saveState).toBe('saved')
  })

  it('a viewer never has anything to save', async () => {
    const { app, editor } = open()
    app.user = { id: 'u2', email: 'v@b.c', name: 'V', role: 'viewer', mustChangePassword: false }
    expect(editor.saveState).toBe('readonly')
    expect(editor.edit((a) => { a.meta = { name: 'x' } })).toBe(false)
    expect(await editor.flushSave()).toBe(true)
    expect(attempts).toBe(0)
  })
})
