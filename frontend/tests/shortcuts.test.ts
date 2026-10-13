import { describe, expect, it } from 'vitest'
import { chord, GROUPS, keyLabel, SHORTCUTS, shortcutText } from '../src/lib/shortcuts'

describe('keyboard shortcuts table', () => {
  it('has unique ids, a group and at least one chord each', () => {
    expect(new Set(SHORTCUTS.map((s) => s.id)).size).toBe(SHORTCUTS.length)
    for (const s of SHORTCUTS) {
      expect(GROUPS, s.id).toContain(s.group)
      expect(s.keys.length, s.id).toBeGreaterThan(0)
      for (const k of s.keys) expect(k.length, s.id).toBeGreaterThan(0)
    }
    for (const g of GROUPS) expect(SHORTCUTS.some((s) => s.group === g), g).toBe(true)
  })

  it('writes chords the way the platform does', () => {
    // jsdom is not a Mac
    expect(keyLabel('Mod')).toBe('Ctrl')
    expect(chord(['Mod', 'Shift', 'Z'])).toBe('Ctrl+Shift+Z')
    expect(shortcutText('palette')).toBe('Ctrl+K')
    expect(shortcutText('undo')).toBe('Ctrl+Z')
    expect(shortcutText('does-not-exist')).toBe('')
  })

  it('the shortcuts the editor handles are documented', () => {
    // the handler of EditorView.vue is the source of truth for these; the table must not lose any of them
    const handled = ['palette', 'shortcuts', 'save', 'escape', 'library', 'undo', 'redo', 'duplicate', 'delete', 'selectAll', 'tool.select', 'tool.connect', 'tool.zone']
    for (const id of handled) expect(SHORTCUTS.map((s) => s.id), id).toContain(id)
  })
})
