import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import type { Architecture } from '../src/api/types'
import { PREFIX, browserStorage, drop, dropUser, judge, keyOf, prune, read, sameArchitecture, write, type Backup, type Retention, type StorageLike } from '../src/lib/unsavedBackup'
import { fixture } from './fixtures'

// The real storage of jsdom, with its real quota: nothing is faked. The only doubles are two storages that misbehave on purpose (one that throws on every call,
// one that accepts a write and keeps nothing), because no real one can be made to do that on demand.

const HOUR = 3_600_000
const keep = 168 * HOUR
const at = (now: number): Retention => ({ now, keepMs: keep })
const T0 = Date.UTC(2026, 9, 1, 12)

const arch: Architecture = fixture('report-spof').architecture
const edited = (): Architecture => ({ ...arch, nodes: arch.nodes.map((n, i) => (i === 0 ? { ...n, name: 'Renamed card' } : n)) })
const backup = (o: Partial<Backup> = {}): Omit<Backup, 'v'> => ({ userId: 'u1', projectId: 'p1', baseRev: 4, savedAt: T0, architecture: edited(), ...o })

beforeEach(() => localStorage.clear())
afterEach(() => localStorage.clear())

describe('keeping a copy', () => {
  it('round-trips the architecture exactly', () => {
    const b = backup()
    expect(write(localStorage, b, at(T0))).toBe(true)
    const back = read(localStorage, 'u1', 'p1', at(T0))
    expect(back).toEqual({ v: 1, ...b })
    expect(back!.architecture).toEqual(edited())
    expect(back!.architecture.nodes[0].name).toBe('Renamed card')
  })

  it('is one entry per user and per project, under a key that says which', () => {
    expect(keyOf('u1', 'p1')).toBe(`${PREFIX}u1:p1`)
    write(localStorage, backup(), at(T0))
    write(localStorage, backup({ projectId: 'p2', baseRev: 9 }), at(T0))
    write(localStorage, backup({ userId: 'u2', baseRev: 1 }), at(T0))
    expect(Object.keys(localStorage).sort()).toEqual([`${PREFIX}u1:p1`, `${PREFIX}u1:p2`, `${PREFIX}u2:p1`])
    expect(read(localStorage, 'u1', 'p1', at(T0))!.baseRev).toBe(4)
    expect(read(localStorage, 'u1', 'p2', at(T0))!.baseRev).toBe(9)
    expect(read(localStorage, 'u2', 'p1', at(T0))!.baseRev).toBe(1)
    expect(read(localStorage, 'u1', 'p3', at(T0))).toBeNull()
  })

  it('keeps only the latest copy of a project', () => {
    write(localStorage, backup({ baseRev: 4 }), at(T0))
    write(localStorage, backup({ baseRev: 5, savedAt: T0 + 1000 }), at(T0 + 1000))
    expect(localStorage.length).toBe(1)
    expect(read(localStorage, 'u1', 'p1', at(T0 + 1000))!.baseRev).toBe(5)
  })

  it('does not hand over the copy of another user, even when it was put under the key of this one', () => {
    write(localStorage, backup({ userId: 'u1' }), at(T0))
    expect(read(localStorage, 'u2', 'p1', at(T0))).toBeNull()
    localStorage.setItem(keyOf('u2', 'p1'), localStorage.getItem(keyOf('u1', 'p1'))!) // a copy that says it is u1's, found where u2 looks
    expect(read(localStorage, 'u2', 'p1', at(T0))).toBeNull()
  })

  it('reads a copy that is not valid JSON, or of another version of the format, or without an architecture, as no copy', () => {
    localStorage.setItem(keyOf('u1', 'p1'), '{not json')
    expect(read(localStorage, 'u1', 'p1', at(T0))).toBeNull()
    localStorage.setItem(keyOf('u1', 'p1'), JSON.stringify({ v: 2, userId: 'u1', projectId: 'p1', baseRev: 1, savedAt: T0, architecture: arch }))
    expect(read(localStorage, 'u1', 'p1', at(T0))).toBeNull()
    localStorage.setItem(keyOf('u1', 'p1'), JSON.stringify({ v: 1, userId: 'u1', projectId: 'p1', baseRev: 1, savedAt: T0 }))
    expect(read(localStorage, 'u1', 'p1', at(T0))).toBeNull()
    localStorage.setItem(keyOf('u1', 'p1'), JSON.stringify({ v: 1, userId: 'u1', projectId: 'p1', baseRev: 1, savedAt: T0, architecture: { nodes: 'no' } }))
    expect(read(localStorage, 'u1', 'p1', at(T0))).toBeNull()
  })
})

describe('how long a copy lives', () => {
  it('goes away after the retention of the settings (`editor.unsavedKeepHours`), and not a moment before', () => {
    write(localStorage, backup(), at(T0))
    expect(read(localStorage, 'u1', 'p1', at(T0 + keep - 1))).not.toBeNull()
    expect(read(localStorage, 'u1', 'p1', at(T0 + keep))).toBeNull()
    expect(localStorage.length).toBe(0) // gone from the storage, not merely hidden
  })

  it('the retention is an argument: a shorter one drops what a longer one keeps', () => {
    write(localStorage, backup(), at(T0))
    expect(read(localStorage, 'u1', 'p1', { now: T0 + 2 * HOUR, keepMs: 3 * HOUR })).not.toBeNull()
    expect(read(localStorage, 'u1', 'p1', { now: T0 + 2 * HOUR, keepMs: 1 * HOUR })).toBeNull()
  })

  it('pruning on the way removes the old copies of other projects and users, the unreadable ones, and nothing else', () => {
    write(localStorage, backup({ projectId: 'old', savedAt: T0 - keep - 1 }), at(T0 - keep - 1))
    write(localStorage, backup({ projectId: 'new', savedAt: T0 }), at(T0))
    write(localStorage, backup({ userId: 'u9', projectId: 'old2', savedAt: T0 - 2 * keep }), at(T0 - 2 * keep))
    localStorage.setItem(`${PREFIX}u1:broken`, '{{{')
    localStorage.setItem(`${PREFIX}u1:future`, JSON.stringify({ v: 7, savedAt: 0 }))
    localStorage.setItem('ia_lang', 'ru')
    prune(localStorage, at(T0))
    expect(Object.keys(localStorage).sort()).toEqual(['ia_lang', `${PREFIX}u1:future`, `${PREFIX}u1:new`].sort())
  })
})

describe('letting go of a copy', () => {
  it('drop removes the copy of one project of one user', () => {
    write(localStorage, backup(), at(T0))
    write(localStorage, backup({ projectId: 'p2' }), at(T0))
    drop(localStorage, 'u1', 'p1')
    expect(read(localStorage, 'u1', 'p1', at(T0))).toBeNull()
    expect(read(localStorage, 'u1', 'p2', at(T0))).not.toBeNull()
  })

  it('signing out removes every copy of that user and no one else\'s, and nothing that is not a copy', () => {
    write(localStorage, backup({ projectId: 'a' }), at(T0))
    write(localStorage, backup({ projectId: 'b' }), at(T0))
    write(localStorage, backup({ userId: 'u2', projectId: 'a' }), at(T0))
    localStorage.setItem('ia_tour_off', '1')
    dropUser(localStorage, 'u1')
    expect(Object.keys(localStorage).sort()).toEqual(['ia_tour_off', `${PREFIX}u2:a`].sort())
  })

  it('a user whose id starts like another\'s does not take the other\'s copies along', () => {
    write(localStorage, backup({ userId: 'u1', projectId: 'a' }), at(T0))
    write(localStorage, backup({ userId: 'u10', projectId: 'a' }), at(T0))
    dropUser(localStorage, 'u1')
    expect(Object.keys(localStorage)).toEqual([`${PREFIX}u10:a`])
  })
})

describe('when the browser will not keep it', () => {
  it('a full storage is a false from write, with the real quota error behind it, and the page does not break', () => {
    // fill it to the brim with real writes: 1 MB pieces until the quota error, then smaller and smaller ones into the room that is left
    let n = 0
    let size = 1024 * 1024
    while (size >= 16) {
      try { localStorage.setItem(`fill${n++}`, 'x'.repeat(size)) } catch { size = Math.floor(size / 4) }
    }
    expect(n, 'the storage of jsdom has a quota').toBeGreaterThan(1)
    expect(() => localStorage.setItem('one-more', 'x'.repeat(64))).toThrow()
    expect(write(localStorage, backup(), at(T0))).toBe(false)
    expect(read(localStorage, 'u1', 'p1', at(T0))).toBeNull()
  })

  it('a storage that throws on every call never throws out of a call', () => {
    const refusing: StorageLike = {
      get length(): number { throw new Error('SecurityError') },
      key() { throw new Error('SecurityError') },
      getItem() { throw new Error('SecurityError') },
      setItem() { throw new Error('QuotaExceededError') },
      removeItem() { throw new Error('SecurityError') },
    }
    expect(write(refusing, backup(), at(T0))).toBe(false)
    expect(read(refusing, 'u1', 'p1', at(T0))).toBeNull()
    expect(() => drop(refusing, 'u1', 'p1')).not.toThrow()
    expect(() => dropUser(refusing, 'u1')).not.toThrow()
    expect(() => prune(refusing, at(T0))).not.toThrow()
  })

  it('no storage at all is no copy and a false', () => {
    expect(write(null, backup(), at(T0))).toBe(false)
    expect(read(null, 'u1', 'p1', at(T0))).toBeNull()
    expect(() => drop(null, 'u1', 'p1')).not.toThrow()
    expect(() => dropUser(null, 'u1')).not.toThrow()
  })

  it('a storage that accepts the write and keeps nothing is not claimed to hold the copy', () => {
    const forgetful: StorageLike = { length: 0, key: () => null, getItem: () => null, setItem: () => undefined, removeItem: () => undefined }
    expect(write(forgetful, backup(), at(T0))).toBe(false)
  })

  it('the browser\'s storage is the storage of the page', () => {
    expect(browserStorage()).toBe(localStorage)
  })
})

describe('what to do with a copy when the project opens', () => {
  const loaded = arch

  it('no copy: nothing', () => {
    expect(judge(null, loaded, 4)).toEqual({ kind: 'none' })
  })

  it('a copy identical to what the server holds is used up (the save got through, only the answer was lost), whatever the order of keys', () => {
    const shuffled = JSON.parse(JSON.stringify(loaded, (_k, v) => (v && typeof v === 'object' && !Array.isArray(v) ? Object.fromEntries(Object.entries(v).reverse()) : v)))
    const b = { v: 1 as const, ...backup({ architecture: shuffled, baseRev: 2 }) }
    expect(judge(b, loaded, 9)).toEqual({ kind: 'same' })
  })

  it('a copy that differs is offered; on the revision it was made on, plainly', () => {
    const b = { v: 1 as const, ...backup({ baseRev: 4 }) }
    expect(judge(b, loaded, 4)).toEqual({ kind: 'offer', moved: false })
  })

  it('a copy made on an older revision than the server holds is STILL offered, with the warning: the server moving on never throws the edits away', () => {
    const b = { v: 1 as const, ...backup({ baseRev: 4 }) }
    expect(judge(b, loaded, 5)).toEqual({ kind: 'offer', moved: true })
    expect(judge(b, loaded, 400)).toEqual({ kind: 'offer', moved: true })
    // and nothing in the storage was touched by asking
    write(localStorage, backup({ baseRev: 4 }), at(T0))
    judge(read(localStorage, 'u1', 'p1', at(T0)), loaded, 400)
    expect(read(localStorage, 'u1', 'p1', at(T0))).not.toBeNull()
  })
})

describe('the same document', () => {
  it('ignores the order of keys and keys without a value, not the order of a list or a value', () => {
    expect(sameArchitecture(arch, JSON.parse(JSON.stringify(arch)))).toBe(true)
    expect(sameArchitecture({ ...arch, meta: { a: 1, b: 2 } } as Architecture, { ...arch, meta: { b: 2, a: 1 } } as Architecture)).toBe(true)
    expect(sameArchitecture({ ...arch, meta: { a: 1, gone: undefined } } as Architecture, { ...arch, meta: { a: 1 } } as Architecture)).toBe(true)
    expect(sameArchitecture(arch, edited())).toBe(false)
    expect(sameArchitecture(arch, { ...arch, nodes: [...arch.nodes].reverse() })).toBe(arch.nodes.length < 2)
    expect(sameArchitecture(arch, { ...arch, nodes: [] })).toBe(false)
  })
})
