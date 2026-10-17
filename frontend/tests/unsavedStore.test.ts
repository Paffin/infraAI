import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import type { Architecture } from '../src/api/types'
import { setLang, t } from '../src/i18n'
import { keyOf, read, write, PREFIX } from '../src/lib/unsavedBackup'
import { useApp } from '../src/stores/app'
import { useDialogs } from '../src/stores/dialogs'
import { useEditor } from '../src/stores/editor'
import { useToasts } from '../src/stores/toasts'
import { useUnsaved } from '../src/stores/unsaved'
import { fixture, realIndex } from './fixtures'

// The offering back of edits that did not reach the server. Real pinia, the real stores, the real storage of jsdom. There is no server at the address of the page,
// so every request fails the way it fails with no network: that is the state these edits are made for, and it is the path of "the save does not get through".

const arch: Architecture = fixture('report-spof').architecture
const renamed = (name: string): Architecture => ({ ...arch, nodes: arch.nodes.map((n, i) => (i === 0 ? { ...n, name } : n)) })
const retention = { now: Date.now(), keepMs: 168 * 3_600_000 }
const opened: ReturnType<typeof useEditor>[] = []
/** The keys of the copies in the storage (`ia_lang` and the like are not copies). */
const copies = () => Object.keys(localStorage).filter((k) => k.startsWith(PREFIX))
const tick = () => new Promise((r) => setTimeout(r, 0))

function setup(userId = 'u1') {
  const app = useApp()
  app.user = { id: userId, email: 'a@b.c', name: 'A', role: 'editor', mustChangePassword: false } as never
  app.boot = { editor: { saveRetryMs: 60000 } } as never
  app.index = realIndex() // laying a document out needs the catalog
  const editor = useEditor()
  opened.push(editor)
  editor.projectId = 'p1'
  editor.arch = arch
  editor.rev = 4
  return { app, editor, unsaved: useUnsaved(), dialogs: useDialogs(), toasts: useToasts() }
}

const backup = (architecture: Architecture, o: { userId?: string; projectId?: string; baseRev?: number } = {}) => ({ userId: 'u1', projectId: 'p1', baseRev: 4, savedAt: Date.now(), architecture, ...o })

beforeEach(() => {
  setActivePinia(createPinia())
  localStorage.clear()
  setLang('ru')
})

afterEach(() => {
  for (const e of opened.splice(0)) e.reset()
  localStorage.clear()
})

describe('leaving the project', () => {
  it('with everything saved leaves quietly', async () => {
    const { unsaved, toasts } = setup()
    await unsaved.settle()
    expect(toasts.items).toEqual([])
    expect(copies()).toEqual([])
  })

  it('with edits the server will not take keeps them in this browser and says so (the edit used to vanish without a word)', async () => {
    const { editor, unsaved, toasts } = setup()
    editor.setMeta('Typed while offline')
    await unsaved.settle() // the save is tried, fails, and the edits are kept
    expect(editor.saveState).toBe('error')
    expect(toasts.items.map((x) => x.kind)).toEqual(['error'])
    expect(toasts.items[0].text).toBe(t('unsaved.left.kept'))
    const copy = read(localStorage, 'u1', 'p1', retention)
    expect(copy!.baseRev).toBe(4)
    expect(copy!.architecture.meta.name).toBe('Typed while offline')
  })

  it('says it plainly when they could not be kept either', async () => {
    const { editor, unsaved, toasts } = setup()
    editor.setMeta('x')
    let n = 0
    let size = 1024 * 1024
    while (size >= 16) {
      try { localStorage.setItem(`fill${n++}`, 'x'.repeat(size)) } catch { size = Math.floor(size / 4) }
    }
    await unsaved.settle()
    expect(toasts.items[0].text).toBe(t('unsaved.left.lost'))
    expect(toasts.items[0].text).not.toBe(t('unsaved.left.kept'))
  })

  it('does nothing after the session has ended: the expiry has already done what could be done', async () => {
    const { app, editor, unsaved, toasts } = setup()
    editor.setMeta('x')
    app.user = null
    await unsaved.settle()
    expect(toasts.items).toEqual([])
    expect(copies()).toEqual([])
  })
})

describe('opening a project that has a copy', () => {
  it('without a copy offers nothing', async () => {
    const { unsaved } = setup()
    await unsaved.check('p1')
    expect(unsaved.offer).toBeNull()
  })

  it('offers a copy that differs, with the edits of the revision it was made on', async () => {
    const { unsaved } = setup()
    write(localStorage, backup(renamed('Typed name')), retention)
    await unsaved.check('p1')
    expect(unsaved.offer).not.toBeNull()
    expect(unsaved.offer!.backup.architecture.nodes[0].name).toBe('Typed name')
    expect(unsaved.offer!.moved).toBe(false)
  })

  it('offers a copy made on an older revision too, with the warning: the edits are not thrown away because the server moved on', async () => {
    const { editor, unsaved } = setup()
    editor.rev = 9
    write(localStorage, backup(renamed('Typed name'), { baseRev: 4 }), retention)
    await unsaved.check('p1')
    expect(unsaved.offer!.moved).toBe(true)
    expect(read(localStorage, 'u1', 'p1', retention)).not.toBeNull()
  })

  it('lets go of a copy that is what the server holds (the save got through, only the answer was lost)', async () => {
    const { unsaved } = setup()
    write(localStorage, backup(JSON.parse(JSON.stringify(arch))), retention)
    await unsaved.check('p1')
    expect(unsaved.offer).toBeNull()
    expect(localStorage.getItem(keyOf('u1', 'p1'))).toBeNull()
  })

  it('does not offer the edits of another user of the same browser', async () => {
    const { unsaved } = setup('u2')
    write(localStorage, backup(renamed('Not yours'), { userId: 'u1' }), retention)
    await unsaved.check('p1')
    expect(unsaved.offer).toBeNull()
    expect(localStorage.getItem(keyOf('u1', 'p1'))).not.toBeNull() // and does not touch them
  })

  it('offers it even when the engine cannot be asked what changed (the list of lines is left out, the offer is not)', async () => {
    const { unsaved } = setup()
    write(localStorage, backup(renamed('Typed name')), retention)
    await unsaved.check('p1')
    expect(unsaved.offer!.diff).toBeNull()
    expect(unsaved.offer!.lines).toEqual([])
  })
})

describe('an offer belongs to the project it was found for', () => {
  it('is gone when the editor moves to another project in the same page, and its buttons do nothing there', async () => {
    const { editor, unsaved, dialogs, toasts } = setup()
    write(localStorage, backup(renamed('Typed name')), retention)
    await unsaved.check('p1')
    const stale = unsaved.offer!
    expect(stale).not.toBeNull()

    editor.projectId = 'p2' // a route update: the editor now shows another project
    expect(unsaved.offer).toBeNull()
    await unsaved.restore()
    await unsaved.discard()
    expect(dialogs.confirmReq).toBeNull() // nothing was asked: there is nothing to answer
    expect(await unsaved.openAsNew(editor.arch, stale)).toBeNull() // and the old offer, held by a button that had not re-rendered yet, cannot make a project of p1's edits
    expect(editor.arch.nodes[0].name).toBe(arch.nodes[0].name) // p2 is untouched: not replaced by p1's edits, not turned into a false conflict
    expect(editor.saveState).toBe('saved')
    expect(toasts.items).toEqual([])
    expect(localStorage.getItem(keyOf('u1', 'p1'))).not.toBeNull() // the copy of p1 waits for p1
  })

  it('is not shown while a project loads, and not shown beside the error of a load that failed', async () => {
    const { editor, unsaved } = setup()
    write(localStorage, backup(renamed('Typed name')), retention)
    await unsaved.check('p1')
    expect(unsaved.offer).not.toBeNull()

    editor.loading = true
    expect(unsaved.offer).toBeNull()
    await unsaved.restore()
    expect(editor.arch.nodes[0].name).toBe(arch.nodes[0].name)

    editor.loading = false
    editor.projectId = 'p2'
    editor.loadError = 'the project could not be loaded'
    expect(unsaved.offer).toBeNull()
  })

  it('a check that is answered after the project was closed does not bring its offer back', async () => {
    const { unsaved } = setup()
    write(localStorage, backup(renamed('Typed name')), retention)
    const checking = unsaved.check('p1') // the engine is asked what the edits change ...
    unsaved.clear() // ... and the person goes elsewhere before it answers
    await checking
    expect(unsaved.offer).toBeNull()
  })

  it('and the check of the project that is open now is not undone by one that was started earlier', async () => {
    const { unsaved } = setup()
    write(localStorage, backup(renamed('Typed name')), retention)
    const first = unsaved.check('p1')
    const second = unsaved.check('p1')
    await Promise.all([first, second])
    expect(unsaved.offer).not.toBeNull()
  })
})

describe('answering the offer', () => {
  it('restore puts the copy in place as one step the person can undo; the copy stays until the save got through', async () => {
    const { editor, unsaved, toasts } = setup()
    write(localStorage, backup(renamed('Typed name')), retention)
    await unsaved.check('p1')
    await unsaved.restore() // there is no server: the save does not get through
    expect(editor.arch.nodes[0].name).toBe('Typed name')
    expect(unsaved.offer).toBeNull()
    expect(editor.saveState).toBe('error')
    expect(localStorage.getItem(keyOf('u1', 'p1'))).not.toBeNull() // used up only once it is saved
    expect(toasts.items.some((x) => x.kind === 'error')).toBe(true)
    editor.undo()
    expect(editor.arch.nodes[0].name).toBe(arch.nodes[0].name)
  })

  it('restore over a project that moved on asks first, and does nothing when the answer is no', async () => {
    const { editor, unsaved, dialogs } = setup()
    editor.rev = 9
    write(localStorage, backup(renamed('Typed name'), { baseRev: 4 }), retention)
    await unsaved.check('p1')
    const done = unsaved.restore()
    await tick()
    expect(dialogs.confirmReq?.danger).toBe(true)
    dialogs.answerConfirm(false)
    await done
    expect(editor.arch.nodes[0].name).toBe(arch.nodes[0].name)
    expect(unsaved.offer).not.toBeNull()
    expect(localStorage.getItem(keyOf('u1', 'p1'))).not.toBeNull()
  })

  it('restore over a project that moved on goes on when the answer is yes', async () => {
    const { editor, unsaved, dialogs } = setup()
    editor.rev = 9
    write(localStorage, backup(renamed('Typed name'), { baseRev: 4 }), retention)
    await unsaved.check('p1')
    const done = unsaved.restore()
    await tick()
    dialogs.answerConfirm(true)
    await done
    expect(editor.arch.nodes[0].name).toBe('Typed name')
  })

  it('discard asks, and then the copy is gone; "no" keeps it', async () => {
    const { unsaved, dialogs } = setup()
    write(localStorage, backup(renamed('Typed name')), retention)
    await unsaved.check('p1')
    const no = unsaved.discard()
    await tick()
    expect(dialogs.confirmReq?.danger).toBe(true)
    dialogs.answerConfirm(false)
    await no
    expect(unsaved.offer).not.toBeNull()
    expect(localStorage.getItem(keyOf('u1', 'p1'))).not.toBeNull()

    const yes = unsaved.discard()
    await tick()
    dialogs.answerConfirm(true)
    await yes
    expect(unsaved.offer).toBeNull()
    expect(Object.keys(localStorage).some((k) => k.startsWith(PREFIX))).toBe(false)
  })

  it('closing the project takes the offer with it and leaves the copy where it is', async () => {
    const { unsaved } = setup()
    write(localStorage, backup(renamed('Typed name')), retention)
    await unsaved.check('p1')
    unsaved.clear()
    expect(unsaved.offer).toBeNull()
    expect(localStorage.getItem(keyOf('u1', 'p1'))).not.toBeNull()
  })

  it('a viewer is not offered edits: they cannot make any', async () => {
    const { app, unsaved } = setup()
    app.user = { id: 'u1', email: 'a@b.c', name: 'A', role: 'viewer', mustChangePassword: false } as never
    write(localStorage, backup(renamed('Typed name')), retention)
    await unsaved.check('p1')
    expect(unsaved.offer).toBeNull()
  })
})

describe('signing out', () => {
  it('asks first when there are copies, says how many, and "no" keeps the copies', async () => {
    const { unsaved, dialogs } = setup()
    write(localStorage, backup(renamed('a')), retention)
    write(localStorage, backup(renamed('b'), { projectId: 'p2' }), retention)
    write(localStorage, backup(renamed('theirs'), { userId: 'u2' }), retention)
    const asked = unsaved.confirmSignOut()
    await tick()
    expect(dialogs.confirmReq?.danger).toBe(true)
    expect(dialogs.confirmReq?.text).toContain(': 2.') // two projects of this user; the copy of the other user is not counted
    dialogs.answerConfirm(false)
    expect(await asked).toBe(false)
    expect(copies()).toHaveLength(3)

    const again = unsaved.confirmSignOut()
    await tick()
    dialogs.answerConfirm(true)
    expect(await again).toBe(true)
  })

  it('does not ask when there is nothing to lose', async () => {
    const { unsaved, dialogs } = setup()
    write(localStorage, backup(renamed('theirs'), { userId: 'u2' }), retention)
    expect(await unsaved.confirmSignOut()).toBe(true)
    expect(dialogs.confirmReq).toBeNull()
  })

  it('removes the copies of that user from the browser and leaves the others', async () => {
    const { app } = setup()
    write(localStorage, backup(renamed('mine')), retention)
    write(localStorage, backup(renamed('theirs'), { userId: 'u2' }), retention)
    await app.logout()
    expect(Object.keys(localStorage).filter((k) => k.startsWith(PREFIX))).toEqual([keyOf('u2', 'p1')])
  })
})
