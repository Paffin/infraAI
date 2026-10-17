import { describe, expect, it } from 'vitest'
import { clear, emptyHistory, record, redo, undo, type History, type HistoryLimits } from '../src/lib/history'

// The documents are plain strings and the clock is a number the test moves: the module has no other input.

const lim: HistoryLimits = { windowMs: 1200, limit: 100 }

/** The document as an editor sees it: a value that edits replace; every call is one edit made at `now`. */
function session(o: HistoryLimits = lim) {
  let h: History<string> = emptyHistory()
  let doc = 'v0'
  return {
    edit(next: string, key: string | undefined, now: number) { h = record(h, doc, key, now, o); doc = next },
    undo() { const r = undo(h, doc); if (r) { h = r.history; doc = r.value } return !!r },
    redo() { const r = redo(h, doc); if (r) { h = r.history; doc = r.value } return !!r },
    get h() { return h },
    get doc() { return doc },
  }
}

describe('merging edits into one undo step', () => {
  it('two edits of the same thing inside the window are one step', () => {
    const s = session()
    s.edit('a', 'name', 0)
    s.edit('ab', 'name', 500)
    expect(s.h.past).toEqual(['v0'])
    s.undo()
    expect(s.doc).toBe('v0')
  })

  it('an edit after the window is a step of its own', () => {
    const s = session()
    s.edit('a', 'name', 0)
    s.edit('ab', 'name', 1200) // exactly the window: not inside it
    expect(s.h.past).toEqual(['v0', 'a'])
  })

  it('the last millisecond inside the window still merges', () => {
    const s = session()
    s.edit('a', 'name', 0)
    s.edit('ab', 'name', 1199)
    expect(s.h.past).toEqual(['v0'])
  })

  it('edits of different things are different steps, even at the same instant', () => {
    const s = session()
    s.edit('a', 'name', 0)
    s.edit('ab', 'role', 0)
    expect(s.h.past).toEqual(['v0', 'a'])
  })

  it('an edit without a key, or with an empty one, is never merged', () => {
    const s = session()
    s.edit('a', undefined, 0)
    s.edit('b', undefined, 1)
    s.edit('c', '', 2)
    s.edit('d', '', 3)
    expect(s.h.past).toEqual(['v0', 'a', 'b', 'c'])
  })

  it('the window slides: it is measured from the last edit, not from the first', () => {
    const s = session()
    s.edit('a', 'name', 0)
    s.edit('ab', 'name', 1100) // 1100 after the first
    s.edit('abc', 'name', 2200) // 1100 after the second, 2200 after the first
    expect(s.h.past).toEqual(['v0'])
    s.edit('abcd', 'name', 3400) // 1200 after the third
    expect(s.h.past).toEqual(['v0', 'abc'])
  })

  it('an edit of another thing moves the window too: coming back to the first thing is a new step', () => {
    const s = session()
    s.edit('a', 'name', 0)
    s.edit('b', 'role', 100)
    s.edit('c', 'name', 200)
    expect(s.h.past).toEqual(['v0', 'a', 'b'])
  })

  it('a window of 0 merges nothing', () => {
    const s = session({ windowMs: 0, limit: 100 })
    s.edit('a', 'name', 5)
    s.edit('ab', 'name', 5)
    s.edit('abc', 'name', 5)
    expect(s.h.past).toEqual(['v0', 'a', 'ab'])
  })

  it('the window is an argument: a longer one merges what a shorter one does not', () => {
    for (const [windowMs, steps] of [[1000, 2], [3000, 1]] as const) {
      const s = session({ windowMs, limit: 100 })
      s.edit('a', 'name', 0)
      s.edit('ab', 'name', 2000)
      expect(s.h.past.length, `window ${windowMs}`).toBe(steps)
    }
  })
})

describe('the number of steps kept', () => {
  it('the oldest step goes first when there are too many', () => {
    const s = session({ windowMs: 0, limit: 3 })
    for (let i = 1; i <= 5; i++) s.edit(`v${i}`, undefined, i)
    expect(s.h.past).toEqual(['v2', 'v3', 'v4'])
    let steps = 0
    while (s.undo()) steps++
    expect(steps).toBe(3)
    expect(s.doc).toBe('v2')
  })

  it('a limit of one keeps one step', () => {
    const s = session({ windowMs: 0, limit: 1 })
    s.edit('a', undefined, 0)
    s.edit('b', undefined, 1)
    expect(s.h.past).toEqual(['a'])
  })

  it('a merged edit pushes nothing, so it cannot push a step out', () => {
    const s = session({ windowMs: 1000, limit: 2 })
    s.edit('a', undefined, 0)
    s.edit('b', 'k', 10)
    s.edit('c', 'k', 20)
    s.edit('d', 'k', 30)
    expect(s.h.past).toEqual(['v0', 'a']) // 'b', 'c' and 'd' are one step; the first one pushed 'a', and nothing after it pushed anything
  })
})

describe('undo and redo', () => {
  it('undo then redo comes back to the same document', () => {
    const s = session()
    s.edit('a', undefined, 0)
    s.edit('b', undefined, 10)
    expect(s.undo()).toBe(true)
    expect(s.doc).toBe('a')
    expect(s.redo()).toBe(true)
    expect(s.doc).toBe('b')
    expect(s.h.future).toEqual([])
  })

  it('a new edit clears the redo steps', () => {
    const s = session()
    s.edit('a', undefined, 0)
    s.undo()
    expect(s.h.future).toEqual(['a'])
    s.edit('b', undefined, 10)
    expect(s.h.future).toEqual([])
    expect(s.redo()).toBe(false)
  })

  it('a merged edit clears them too: it is a new edit, only its undo step is shared', () => {
    // a history that still has a redo step, and an edit that merges by key and time
    const h: History<string> = { past: ['v0'], future: ['stale'], lastKey: 'k', lastAt: 0 }
    const after = record(h, 'x', 'k', 10, lim)
    expect(after.past).toEqual(['v0'])
    expect(after.future).toEqual([])
  })

  it('undo and redo on an empty history are no-ops that say so', () => {
    const s = session()
    expect(s.undo()).toBe(false)
    expect(s.redo()).toBe(false)
    expect(s.h).toEqual(emptyHistory())
    expect(s.doc).toBe('v0')
  })

  it('an undo forgets what the last edit was: the next edit of the same thing is a new step', () => {
    const s = session()
    s.edit('a', 'name', 0)
    s.edit('b', 'role', 5000)
    s.undo()
    expect(s.h.lastKey).toBe('')
    s.edit('c', 'role', 5010)
    expect(s.h.past).toEqual(['v0', 'a'])
    s.undo()
    s.redo()
    expect(s.h.lastKey).toBe('')
  })

  it('does not change the history it is given', () => {
    const h: History<string> = { past: ['a', 'b'], future: ['z'], lastKey: 'k', lastAt: 7 }
    const copy = JSON.parse(JSON.stringify(h))
    record(h, 'c', 'k', 8, lim)
    undo(h, 'c')
    redo(h, 'c')
    expect(h).toEqual(copy)
  })

  it('clear is an empty history', () => {
    expect(clear()).toEqual(emptyHistory())
    expect(emptyHistory().past).not.toBe(emptyHistory().past)
  })
})
