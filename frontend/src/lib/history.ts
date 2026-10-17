// The undo history of the editor as plain functions: no Vue, no clock, no storage. The time comes in as an argument and so do the limits, so a test
// decides what "a second later" is without waiting for it. The store (`stores/editor.ts`) keeps one value of this type and replaces it after every edit.

/** The snapshots before the present (`past`, the latest last) and after it (`future`, the next redo last), and what the last edit was. */
export interface History<T> {
  past: T[]
  future: T[]
  /** What the last edit was about (`node:n1:name`); '' when the last step was not an edit that can be merged (an undo, a redo, an edit without a key). */
  lastKey: string
  /** When the last edit was made, ms. The merge window slides: it is measured from here, and every edit, merged or not, moves it. */
  lastAt: number
}

export interface HistoryLimits {
  /** Edits with the same key closer together than this are one step (`editor.coalesceMs`). 0 merges nothing. */
  windowMs: number
  /** How many steps are kept; the oldest is dropped (`editor.historySteps`). */
  limit: number
}

export const emptyHistory = <T>(): History<T> => ({ past: [], future: [], lastKey: '', lastAt: 0 })

/**
 * Record an edit: `current` is what the document was BEFORE the edit. With the same non-empty `key` as the previous edit and less than `windowMs` after it,
 * the edit joins the previous step (nothing is pushed); otherwise `current` becomes a step to go back to, and the oldest step goes when there are more than
 * `limit`. Either way the redo steps are gone: a new edit starts a new future.
 */
export function record<T>(h: History<T>, current: T, key: string | undefined, now: number, o: HistoryLimits): History<T> {
  const merge = !!key && key === h.lastKey && now - h.lastAt < o.windowMs
  let past = h.past
  if (!merge) {
    past = [...h.past, current]
    if (past.length > o.limit) past = past.slice(past.length - o.limit)
  }
  return { past, future: [], lastKey: key ?? '', lastAt: now }
}

/** One step back: `value` is the document to show now, `current` goes to the redo steps. Nothing to go back to: null. */
export function undo<T>(h: History<T>, current: T): { history: History<T>; value: T } | null {
  if (!h.past.length) return null
  const value = h.past[h.past.length - 1]
  return { value, history: { past: h.past.slice(0, -1), future: [...h.future, current], lastKey: '', lastAt: h.lastAt } }
}

/** One step forward again. Nothing to go forward to: null. */
export function redo<T>(h: History<T>, current: T): { history: History<T>; value: T } | null {
  if (!h.future.length) return null
  const value = h.future[h.future.length - 1]
  return { value, history: { past: [...h.past, current], future: h.future.slice(0, -1), lastKey: '', lastAt: h.lastAt } }
}

/** Forget everything (a project was opened, a whole document was put in place of the old one). */
export const clear = <T>(): History<T> => emptyHistory<T>()
