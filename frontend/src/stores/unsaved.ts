import { defineStore } from 'pinia'
import { computed, shallowRef } from 'vue'
import * as api from '@/api/endpoints'
import type { ArchDiff, Architecture } from '@/api/types'
import { t } from '@/i18n'
import { diffLines, type DiffLine } from '@/lib/diff'
import { copyName, errorMessage } from '@/lib/format'
import { browserStorage, countUser, drop, judge, read, write, type Backup, type Retention } from '@/lib/unsavedBackup'
import { useApp } from './app'
import { useDialogs } from './dialogs'
import { useEditor } from './editor'
import { useToasts } from './toasts'

/** Edits that did not reach the server, found in this browser when their project opened. */
export interface Offer {
  backup: Backup
  /** What they change against what the server holds, as lines to read (the first ones; `more` is how many are left out). */
  lines: DiffLine[]
  more: number
  diff: ArchDiff | null
  /** The server has a newer revision than the one the edits were made on: restoring them overwrites what somebody saved since. */
  moved: boolean
}

/** What happened to the unsaved edits when the session ended: nothing was unsaved, they were kept in this browser, or they could not be kept. */
export type Outcome = 'none' | 'kept' | 'lost'

/** Marks the comparison view that shows the offered copy (a `Preview` of a comparison has no use for the id of a proposal). */
const PREVIEW_MARK = 'unsaved'

/**
 * The copy of unsaved edits kept in this browser (`lib/unsavedBackup.ts`): where it is made (a session that ends under an open editor, the page left while the
 * save cannot get through), and where it is offered back (the project opens) and used up. The rules of the storage are in the lib; the rules of WHEN are here.
 */
export const useUnsaved = defineStore('unsaved', () => {
  const app = useApp()
  const editor = useEditor()
  const toasts = useToasts()
  const dialogs = useDialogs()

  /** The offer that was made when a project opened; what the page and the buttons see is `offer`, which is it only for the project that is on the screen. */
  const held = shallowRef<Offer | null>(null)
  /**
   * The offer, for the project on the screen and no other: it is gone the moment another project (or the same one again) is being loaded, and stays gone when that
   * load fails. The buttons act on this, so they cannot restore, discard or copy the edits of one project into another; that holds by construction, not by the
   * order in which the view happens to clear the offer.
   */
  const offer = computed(() => {
    const o = held.value
    return o && !editor.loading && o.backup.projectId === editor.projectId ? o : null
  })
  /** Which `check` is the latest: an answer of an earlier one (or of one that was cleared) must not bring an offer back. */
  let checking = 0
  /** The user the open project belongs to, kept from the moment it opened: after the session has ended the store of the application no longer knows who it was. */
  let owner = ''
  /** The edits of the open project were put elsewhere (a new project): leaving it is not a loss, and a copy would offer them a second time. */
  let handedOver = false

  const retention = (): Retention => ({ now: Date.now(), keepMs: app.editorCfg.unsavedKeepHours * 3_600_000 })
  /** The document on the screen is the offered copy (it can be looked at on the diagram). */
  const showing = computed(() => offer.value !== null && editor.preview?.proposalId === PREVIEW_MARK)

  /**
   * Make the copy of the open project's working document. True only when it is really in the storage. The user comes from the store of the application, or, when
   * the session is already gone, from the moment the project opened.
   */
  function stash(): boolean {
    const userId = app.user?.id || owner
    if (!userId || !editor.projectId) return false
    return write(browserStorage(), { userId, projectId: editor.projectId, baseRev: editor.rev, savedAt: Date.now(), architecture: editor.arch }, retention())
  }

  /** The session ended (a 401): keep what is not on the server. Called BEFORE the application forgets the user. */
  function onSessionExpired(): Outcome {
    if (!editor.projectId || editor.loading || editor.saveState === 'saved' || editor.saveState === 'readonly') return 'none'
    return stash() ? 'kept' : 'lost'
  }

  /**
   * The page is being left (another project, the list of projects, the admin panel). What cannot be stored on the server is kept in this browser, and the person is
   * told so: before this the edit vanished without a word.
   */
  async function settle(): Promise<void> {
    if (!editor.projectId || !app.user) return // no session: the expiry did what could be done
    if (handedOver) { handedOver = false; return }
    if (await editor.flushSave()) return
    toasts.error(t(stash() ? 'unsaved.left.kept' : 'unsaved.left.lost'))
  }

  /** A project has just opened: is there a copy of edits that did not get through? Nothing, or an offer to restore. */
  async function check(projectId: string): Promise<void> {
    const mine = ++checking
    held.value = null
    handedOver = false
    if (!app.user || !app.canEdit) return
    owner = app.user.id
    const storage = browserStorage()
    const found = read(storage, owner, projectId, retention())
    const verdict = judge(found, editor.arch, editor.rev)
    if (verdict.kind === 'same') drop(storage, owner, projectId)
    if (verdict.kind !== 'offer' || !found) return
    let diff: ArchDiff | null = null
    try { diff = await api.engine.diff(editor.arch, found.architecture) } catch { /* the offer is made without the list of lines */ }
    if (mine !== checking || editor.projectId !== projectId) return // the person has gone elsewhere (or the project was closed) while the engine was asked
    const lines = diff ? diffLines(diff, 1) : []
    const shown = app.editorCfg.unsavedOfferLines
    held.value = { backup: found, lines: lines.slice(0, shown), more: Math.max(0, lines.length - shown), diff, moved: verdict.moved }
  }

  /** Put the copy in place of the document: one step of the history (the person can undo it), stored by the autosave; the copy is let go once the save got through. */
  async function restore(): Promise<void> {
    const o = offer.value
    if (!o) return
    if (o.moved && !(await dialogs.confirm({ title: t('unsaved.moved.title'), text: t('unsaved.moved.confirm'), confirmLabel: t('unsaved.restore.anyway'), danger: true }))) return
    if (offer.value !== o) return // answered elsewhere while the question was open
    editor.setPreview(null)
    editor.replaceArchitecture(o.backup.architecture)
    held.value = null
    if (await editor.flushSave()) {
      drop(browserStorage(), o.backup.userId, o.backup.projectId)
      toasts.ok(t('unsaved.restored'))
    } else toasts.error(t('unsaved.restored.unsaved'))
  }

  /** Throw the copy away (after asking: it is the only one). */
  async function discard(): Promise<void> {
    const o = offer.value
    if (!o) return
    if (!(await dialogs.confirm({ title: t('unsaved.discard.title'), text: t('unsaved.discard.confirm'), confirmLabel: t('unsaved.discard'), danger: true }))) return
    if (showing.value) editor.setPreview(null)
    drop(browserStorage(), o.backup.userId, o.backup.projectId)
    if (offer.value === o) held.value = null
  }

  /** Look at what the copy changes, drawn on the diagram (the comparison view); again to go back. */
  function toggleView() {
    const o = offer.value
    if (!o || !o.diff) return
    if (showing.value) editor.setPreview(null)
    else editor.setPreview({ kind: 'compare', label: t('unsaved.label'), proposalId: PREVIEW_MARK, base: editor.arch, arch: o.backup.architecture, diff: o.diff })
  }

  /**
   * The edits become a project of their own: the copy of the offer, or the working document of a project whose edits can no longer be stored in it (a conflict).
   * Returns the id of the new project, or null when it could not be made (the error is said).
   */
  async function openAsNew(architecture: Architecture, offered: Offer | null): Promise<string | null> {
    if (offered && offer.value !== offered) return null // the copy of a project that is not on the screen: it is not this document's to turn into a project
    try {
      const name = copyName(editor.project?.name ?? '')
      const made = await api.projects.create({ name, architecture })
      if (offered) drop(browserStorage(), offered.backup.userId, offered.backup.projectId)
      handedOver = true
      held.value = null
      return made.id
    } catch (e) {
      toasts.error(errorMessage(e))
      return null
    }
  }

  /** The project closes, or another one is about to open: the offer goes (the copy stays in the storage until it is used up), and so does any check still under way. */
  function clear() {
    checking++
    held.value = null
    handedOver = false
  }

  /**
   * Signing out removes the copies of this user from the browser (the next person at it must not find them), so the person is told before, if there are any:
   * edits that never reached the server are not something to lose in a click on "Sign out". True when they may go on.
   */
  async function confirmSignOut(): Promise<boolean> {
    if (!app.user) return true
    const n = countUser(browserStorage(), app.user.id, retention())
    if (!n) return true
    return dialogs.confirm({ title: t('unsaved.signout.title'), text: t('unsaved.signout.confirm', { n }), confirmLabel: t('unsaved.signout.go'), danger: true })
  }

  return { offer, showing, stash, onSessionExpired, settle, check, restore, discard, toggleView, openAsNew, clear, confirmSignOut }
})
