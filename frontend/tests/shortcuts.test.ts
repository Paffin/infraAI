import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { blocked, chord, GROUPS, keyLabel, matchShortcut, parseChord, repeats, SHORTCUTS, shortcutText, type Shortcut } from '../src/lib/shortcuts'

const table: readonly Shortcut[] = SHORTCUTS
const windowOnes = table.filter((s) => s.scope === 'window')

/** The Russian layout over the Latin letters (ЙЦУКЕН): the letter a key makes there, by the place of the key. */
const RU: Record<string, string> = { q: 'й', w: 'ц', e: 'у', r: 'к', t: 'е', y: 'н', u: 'г', i: 'ш', o: 'щ', p: 'з', a: 'ф', s: 'ы', d: 'в', f: 'а', g: 'п', h: 'р', j: 'о', k: 'л', l: 'д', z: 'я', x: 'ч', c: 'с', v: 'м', b: 'и', n: 'т', m: 'ь' }

/** The key press a chord (as the help writes it) is made by, on a layout that makes the letters of the chord. */
function press(keys: readonly string[], russian = false): KeyboardEvent {
  const p = parseChord(keys)!
  const letter = /^[a-z]$/.test(p.key)
  return new KeyboardEvent('keydown', {
    key: russian && letter ? RU[p.key] : p.key,
    code: p.code,
    ctrlKey: p.mod,
    shiftKey: p.shift === true,
  })
}

describe('keyboard shortcuts table', () => {
  it('has unique ids, a group, a scope and at least one chord each', () => {
    expect(new Set(table.map((s) => s.id)).size).toBe(table.length)
    for (const s of table) {
      expect(GROUPS, s.id).toContain(s.group)
      expect(['window', 'card', 'pointer'], s.id).toContain(s.scope)
      expect(s.keys.length, s.id).toBeGreaterThan(0)
      for (const k of s.keys) expect(k.length, s.id).toBeGreaterThan(0)
    }
    for (const g of GROUPS) expect(table.some((s) => s.group === g), g).toBe(true)
  })

  it('is long enough to be the keyboard of the editor: 21 entries before, now the zoom, fit, focus, layout, export, matrix and findings too', () => {
    expect(table.length).toBeGreaterThanOrEqual(30)
    for (const id of ['zoomIn', 'zoomOut', 'fit', 'focus', 'layout', 'export', 'matrix', 'findingNext', 'findingPrev']) expect(table.map((s) => s.id), id).toContain(id)
    // the keys of a focused card are in the help, and are not the page's
    for (const id of ['canvas.move', 'canvas.select', 'canvas.extend', 'canvas.inspect']) expect(table.find((s) => s.id === id)?.scope, id).toBe('card')
  })

  it('writes chords the way the platform does', () => {
    // jsdom is not a Mac
    expect(keyLabel('Mod')).toBe('Ctrl')
    expect(chord(['Mod', 'Shift', 'Z'])).toBe('Ctrl+Shift+Z')
    expect(shortcutText('palette')).toBe('Ctrl+K')
    expect(shortcutText('undo')).toBe('Ctrl+Z')
    expect(shortcutText('findingNext')).toBe('N')
    expect(shortcutText('does-not-exist')).toBe('')
  })

  it('the shortcuts the editor handled before are still there', () => {
    const handled = ['palette', 'shortcuts', 'save', 'escape', 'library', 'foldLibrary', 'foldRight', 'undo', 'redo', 'duplicate', 'delete', 'selectAll', 'tool.select', 'tool.connect', 'tool.zone', 'play', 'step']
    for (const id of handled) expect(windowOnes.map((s) => s.id), id).toContain(id)
  })

  it('no two shortcuts of the page share a chord; "[" and "]" stay the folding of the panels', () => {
    const owner = new Map<string, string>()
    for (const s of windowOnes) {
      for (const keys of [...s.keys, ...(s.also ?? [])]) {
        const p = parseChord(keys)
        expect(p, `${s.id} ${keys.join('+')} is a chord of the page`).not.toBeNull()
        const at = `${p!.mod ? 'Mod+' : ''}${p!.shift === true ? 'Shift+' : ''}${p!.key}`
        // Space and the arrows are told apart by what the page is doing (a failure on screen) and by where the focus is, not by the chord: `play` and `step` are the only ones with them
        expect(owner.get(at) ?? s.id, `${at} is ${owner.get(at)} and ${s.id}`).toBe(s.id)
        owner.set(at, s.id)
      }
    }
    expect(owner.get('[')).toBe('foldLibrary')
    expect(owner.get(']')).toBe('foldRight')
    expect(owner.get('n')).toBe('findingNext')
    expect(owner.get('p')).toBe('findingPrev')
  })
})

describe('the table is the dispatcher', () => {
  it('every shortcut of the page is recognised from its own chord, and from every alternative chord', () => {
    for (const s of windowOnes) {
      for (const keys of [...s.keys, ...(s.also ?? [])]) {
        expect(matchShortcut(press(keys)), `${s.id}: ${keys.join('+')}`).toBe(s.id)
      }
    }
  })

  it('on the Russian layout the letters are the same keys: the place of the key says which it is', () => {
    let letters = 0
    for (const s of windowOnes) {
      for (const keys of [...s.keys, ...(s.also ?? [])]) {
        const p = parseChord(keys)!
        if (!/^[a-z]$/.test(p.key)) continue
        letters++
        const ev = press(keys, true)
        expect(ev.key, s.id).not.toBe(p.key) // the key makes a Cyrillic letter
        expect(matchShortcut(ev), `${s.id}: ${keys.join('+')} as ${ev.key}`).toBe(s.id)
      }
    }
    expect(letters).toBeGreaterThanOrEqual(15)
    // the ones that were dead on this layout before: the tools, undo, the palette
    expect(matchShortcut(new KeyboardEvent('keydown', { key: 'с', code: 'KeyC' }))).toBe('tool.connect')
    expect(matchShortcut(new KeyboardEvent('keydown', { key: 'м', code: 'KeyV' }))).toBe('tool.select')
    expect(matchShortcut(new KeyboardEvent('keydown', { key: 'л', code: 'KeyK', ctrlKey: true }))).toBe('palette')
    expect(matchShortcut(new KeyboardEvent('keydown', { key: 'я', code: 'KeyZ', ctrlKey: true }))).toBe('undo')
    expect(matchShortcut(new KeyboardEvent('keydown', { key: 'я', code: 'KeyZ', ctrlKey: true, shiftKey: true }))).toBe('redo')
    // the brackets are folded by the place of the key too (`х` and `ъ` on that layout)
    expect(matchShortcut(new KeyboardEvent('keydown', { key: 'х', code: 'BracketLeft' }))).toBe('foldLibrary')
    expect(matchShortcut(new KeyboardEvent('keydown', { key: 'ъ', code: 'BracketRight' }))).toBe('foldRight')
  })

  it('a key that is a letter of the Latin alphabet is that letter, wherever the key is (a Dvorak or AZERTY keyboard types the letter it shows)', () => {
    // on this layout the key of "C" in the QWERTY row makes "j": that is not the connect tool, and a "c" made by another key is
    expect(matchShortcut(new KeyboardEvent('keydown', { key: 'j', code: 'KeyC' }))).toBeNull()
    expect(matchShortcut(new KeyboardEvent('keydown', { key: 'c', code: 'KeyJ' }))).toBe('tool.connect')
  })

  it('the numeric keypad zooms too, and the digit fits', () => {
    expect(matchShortcut(new KeyboardEvent('keydown', { key: '+', code: 'NumpadAdd' }))).toBe('zoomIn')
    expect(matchShortcut(new KeyboardEvent('keydown', { key: '+', code: 'Equal', shiftKey: true }))).toBe('zoomIn')
    expect(matchShortcut(new KeyboardEvent('keydown', { key: '=', code: 'Equal' }))).toBe('zoomIn')
    expect(matchShortcut(new KeyboardEvent('keydown', { key: '-', code: 'NumpadSubtract' }))).toBe('zoomOut')
    expect(matchShortcut(new KeyboardEvent('keydown', { key: '0', code: 'Numpad0' }))).toBe('fit')
    expect(matchShortcut(new KeyboardEvent('keydown', { key: 'Backspace', code: 'Backspace' }))).toBe('delete')
    expect(matchShortcut(new KeyboardEvent('keydown', { key: '?', code: 'Slash', shiftKey: true }))).toBe('shortcuts')
  })

  it('does not take a key that only looks like one: another modifier, a letter with Ctrl that has no meaning, Alt', () => {
    expect(matchShortcut(new KeyboardEvent('keydown', { key: 'k', code: 'KeyK' }))).toBeNull() // K alone is not the palette
    expect(matchShortcut(new KeyboardEvent('keydown', { key: 'n', code: 'KeyN', ctrlKey: true }))).toBeNull() // Ctrl+N is the browser's
    expect(matchShortcut(new KeyboardEvent('keydown', { key: 'f', code: 'KeyF', ctrlKey: true }))).toBeNull() // Ctrl+F is the browser's
    expect(matchShortcut(new KeyboardEvent('keydown', { key: 'z', code: 'KeyZ', altKey: true }))).toBeNull()
    expect(matchShortcut(new KeyboardEvent('keydown', { key: 'F5', code: 'F5' }))).toBeNull()
    expect(matchShortcut(new KeyboardEvent('keydown', { key: 'Tab', code: 'Tab' }))).toBeNull()
    expect(matchShortcut(new KeyboardEvent('keydown', { key: 'Enter', code: 'Enter' }))).toBeNull() // Enter is the card's, not the page's
  })

  it('knows where a shortcut does not work: typing, a dialog, a control that takes the key itself', () => {
    const free = { typing: false, control: false, dialog: false }
    expect(blocked('tool.connect', free)).toBe(false)
    expect(blocked('tool.connect', { ...free, typing: true })).toBe(true)
    expect(blocked('tool.connect', { ...free, dialog: true })).toBe(true)
    // the palette and save are for every place
    expect(blocked('palette', { typing: true, control: true, dialog: true })).toBe(false)
    expect(blocked('save', { typing: true, control: false, dialog: true })).toBe(false)
    // a button takes Space, a slider the arrows, so the page's play and step wait; the single letters wait for a control too
    expect(blocked('play', { ...free, control: true })).toBe(true)
    expect(blocked('step', { ...free, control: true })).toBe(true)
    expect(blocked('findingNext', { ...free, control: true })).toBe(true)
    expect(blocked('undo', { ...free, control: true })).toBe(false)
    expect(blocked('escape', { ...free, control: true })).toBe(false)
  })

  it('only the keys that mean "again" repeat while held', () => {
    for (const id of ['zoomIn', 'zoomOut', 'undo', 'redo', 'step']) expect(repeats(id as never), id).toBe(true)
    for (const id of ['palette', 'save', 'export', 'matrix', 'layout', 'tool.connect', 'delete']) expect(repeats(id as never), id).toBe(false)
  })

  it('EditorView gives every shortcut of the page an action: its table is a Record over the ids, so that a missing one does not compile', () => {
    const src = readFileSync(join(process.cwd(), 'src/views/EditorView.vue'), 'utf8')
    expect(src).toMatch(/Record<WindowShortcutId,/)
    // and says so in words the day somebody loosens the type
    const body = src.slice(src.indexOf('const actions'), src.indexOf('function onKey'))
    for (const s of windowOnes) expect(body, s.id).toMatch(new RegExp(`(^|\\n)\\s+['"]?${s.id.replace('.', '\\.')}['"]?:`))
  })
})
