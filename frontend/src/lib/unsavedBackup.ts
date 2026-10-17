import type { Architecture } from '@/api/types'

// A copy of the edits that did not reach the server, kept in this browser: when the session ended under an open editor, or the person left the page while the
// connection was gone. It is not an offline mode (the editor needs the server for every check, ADR-0001): it is the one thing that stops an edit from
// vanishing without a word when the save could not be made.
//
// The rules, each of them a test (`tests/unsavedBackup.test.ts`):
//  * one entry per user AND project: `ia_unsaved:<userId>:<projectId>`; nobody reads the copy of another user of the same browser;
//  * a copy is dropped only when it is used up: restored and saved, thrown away by the person, made a project of its own ("Open as a new project"), identical
//    to what the server holds, signed out (after the person was told how many there are), or too old; NEVER because the server has a newer revision (those are
//    exactly the edits the person would otherwise lose: they decide, not the code);
//  * every access to the storage is guarded: a full or disabled storage (private window, quota) is a `false` and a `null`, never an exception, and `write`
//    says whether the copy is really there, because the page tells the person so.

export const PREFIX = 'ia_unsaved:'
const VERSION = 1

export interface Backup {
  v: 1
  userId: string
  projectId: string
  /** The revision of the server's draft the edits were made on. */
  baseRev: number
  /** When the copy was made, ms since the epoch. */
  savedAt: number
  architecture: Architecture
}

/** What of `Storage` is used (so that a test can hand over the real `localStorage` of jsdom, with its real quota). */
export type StorageLike = Pick<Storage, 'getItem' | 'setItem' | 'removeItem' | 'key' | 'length'>

export interface Retention {
  /** The present, ms. An argument, so that a test decides what "a week later" is. */
  now: number
  /** How long a copy is kept, ms (`editor.unsavedKeepHours`). */
  keepMs: number
}

/** The storage of the browser, or null where it cannot be had (the getter itself can throw). */
export function browserStorage(): StorageLike | null {
  try { return typeof localStorage === 'undefined' ? null : localStorage } catch { return null }
}

export const keyOf = (userId: string, projectId: string): string => `${PREFIX}${userId}:${projectId}`

function parse(raw: string | null): Backup | 'corrupt' | 'foreign' | null {
  if (raw === null) return null
  let x: unknown
  try { x = JSON.parse(raw) } catch { return 'corrupt' }
  if (!x || typeof x !== 'object') return 'corrupt'
  const b = x as Partial<Backup>
  if (b.v !== VERSION) return 'foreign' // a copy of another version of the format: not ours to read, not ours to delete
  const a = b.architecture as Architecture | undefined
  if (typeof b.userId !== 'string' || typeof b.projectId !== 'string' || typeof b.baseRev !== 'number' || typeof b.savedAt !== 'number'
    || !a || typeof a !== 'object' || !Array.isArray(a.nodes) || !Array.isArray(a.edges) || !Array.isArray(a.zones)) return 'corrupt'
  return b as Backup
}

/** Remove the copies that are too old and the ones nobody can read. Every other entry of the storage is left alone. */
export function prune(storage: StorageLike, o: Retention): void {
  try {
    const gone: string[] = []
    for (let i = 0; i < storage.length; i++) {
      const k = storage.key(i)
      if (!k || !k.startsWith(PREFIX)) continue
      const b = parse(storage.getItem(k))
      if (b === 'corrupt' || (b && b !== 'foreign' && o.now - b.savedAt >= o.keepMs)) gone.push(k)
    }
    for (const k of gone) storage.removeItem(k)
  } catch { /* a storage that throws has nothing to prune */ }
}

/**
 * Keep the copy. Returns true only when it is really in the storage: it is read back, because a storage can accept a write and still not hold it
 * (a quota error is thrown, but a private window may silently keep nothing). The older copies are pruned first, which also frees room.
 */
export function write(storage: StorageLike | null, b: Omit<Backup, 'v'>, o: Retention): boolean {
  if (!storage) return false
  try {
    prune(storage, o)
    const text = JSON.stringify({ v: VERSION, ...b } satisfies Backup)
    const key = keyOf(b.userId, b.projectId)
    storage.setItem(key, text)
    return storage.getItem(key) === text
  } catch {
    return false
  }
}

/** The copy of this user for this project, or null: none, too old (and then removed), unreadable, of another format, or of somebody else. */
export function read(storage: StorageLike | null, userId: string, projectId: string, o: Retention): Backup | null {
  if (!storage) return null
  try {
    prune(storage, o)
    const b = parse(storage.getItem(keyOf(userId, projectId)))
    return b && b !== 'corrupt' && b !== 'foreign' && b.userId === userId && b.projectId === projectId ? b : null
  } catch {
    return null
  }
}

/** How many copies of this user are in the storage (signing out removes them all, so the person is told how many). Old and unreadable ones are pruned first: nobody could miss those. */
export function countUser(storage: StorageLike | null, userId: string, o: Retention): number {
  if (!storage) return 0
  try {
    prune(storage, o)
    const mine = `${PREFIX}${userId}:`
    let n = 0
    for (let i = 0; i < storage.length; i++) if (storage.key(i)?.startsWith(mine)) n++
    return n
  } catch {
    return 0
  }
}

export function drop(storage: StorageLike | null, userId: string, projectId: string): void {
  try { storage?.removeItem(keyOf(userId, projectId)) } catch { /* nothing to drop in a storage that throws */ }
}

/** Every copy of one user (signing out). The copies of other users of the browser are not touched. */
export function dropUser(storage: StorageLike | null, userId: string): void {
  if (!storage) return
  try {
    const mine = `${PREFIX}${userId}:`
    const gone: string[] = []
    for (let i = 0; i < storage.length; i++) {
      const k = storage.key(i)
      if (k && k.startsWith(mine)) gone.push(k)
    }
    for (const k of gone) storage.removeItem(k)
  } catch { /* as above */ }
}

function canonical(v: unknown): unknown {
  if (Array.isArray(v)) return v.map(canonical)
  if (v && typeof v === 'object') {
    const out: Record<string, unknown> = {}
    for (const k of Object.keys(v).sort()) {
      const x = (v as Record<string, unknown>)[k]
      if (x !== undefined) out[k] = canonical(x)
    }
    return out
  }
  return v
}

/** The same document? Key order does not matter (the database hands objects back in an order of its own); the order of a list does. */
export function sameArchitecture(a: Architecture, b: Architecture): boolean {
  return JSON.stringify(canonical(a)) === JSON.stringify(canonical(b))
}

/**
 * What to do with the copy of a project that has just been opened: `none` (no copy), `same` (the server holds exactly these edits: the save did get through, and
 * only the answer was lost; the copy is used up), or `offer` (the person decides). `moved`: the server has a newer revision than the one the edits were made on,
 * so restoring them would overwrite what somebody else saved since. That makes the offer more careful; it never makes the copy go away.
 */
export type Verdict = { kind: 'none' } | { kind: 'same' } | { kind: 'offer'; moved: boolean }

export function judge(b: Backup | null, loaded: Architecture, serverRev: number): Verdict {
  if (!b) return { kind: 'none' }
  if (sameArchitecture(b.architecture, loaded)) return { kind: 'same' }
  return { kind: 'offer', moved: serverRev !== b.baseRev }
}
