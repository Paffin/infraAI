/** Keyboard shortcuts of the editor, as data: the help window, the palette and the tooltips read this list, the
 *  handler in `EditorView.vue` implements it (tests/shortcuts.test.ts keeps the two honest). */

export type ShortcutGroup = 'general' | 'edit' | 'tools' | 'canvas'

export interface Shortcut {
  /** key of the description (`kb.<id>`) */
  id: string
  group: ShortcutGroup
  /** alternatives; each is a chord written with the names below (`Mod` = Ctrl, or ⌘ on macOS) */
  keys: string[][]
}

export const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent || '')

/** How a key is written on this platform. */
export function keyLabel(k: string): string {
  if (k === 'Mod') return isMac ? '⌘' : 'Ctrl'
  if (k === 'Shift') return isMac ? '⇧' : 'Shift'
  if (k === 'Alt') return isMac ? '⌥' : 'Alt'
  return k
}

export const chord = (keys: string[]): string => keys.map(keyLabel).join(isMac ? '' : '+')

export const SHORTCUTS: Shortcut[] = [
  { id: 'palette', group: 'general', keys: [['Mod', 'K']] },
  { id: 'shortcuts', group: 'general', keys: [['?']] },
  { id: 'save', group: 'general', keys: [['Mod', 'S']] },
  { id: 'escape', group: 'general', keys: [['Esc']] },
  { id: 'library', group: 'general', keys: [['/']] },
  { id: 'undo', group: 'edit', keys: [['Mod', 'Z']] },
  { id: 'redo', group: 'edit', keys: [['Mod', 'Shift', 'Z'], ['Mod', 'Y']] },
  { id: 'duplicate', group: 'edit', keys: [['Mod', 'D']] },
  { id: 'delete', group: 'edit', keys: [['Del']] },
  { id: 'selectAll', group: 'edit', keys: [['Mod', 'A']] },
  { id: 'tool.select', group: 'tools', keys: [['V']] },
  { id: 'tool.connect', group: 'tools', keys: [['C']] },
  { id: 'tool.zone', group: 'tools', keys: [['Z']] },
  { id: 'multi.click', group: 'canvas', keys: [['Shift', 'click']] },
  { id: 'multi.box', group: 'canvas', keys: [['Shift', 'drag']] },
  { id: 'pan', group: 'canvas', keys: [['drag']] },
  { id: 'zoom', group: 'canvas', keys: [['Mod', 'scroll']] },
]

export const GROUPS: ShortcutGroup[] = ['general', 'edit', 'tools', 'canvas']

/** `Ctrl+K` for a shortcut id (first alternative): used in tooltips and palette hints. */
export function shortcutText(id: string): string {
  const s = SHORTCUTS.find((x) => x.id === id)
  return s ? chord(s.keys[0]) : ''
}
