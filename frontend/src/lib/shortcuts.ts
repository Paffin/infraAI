/**
 * Keyboard shortcuts of the editor, as data. The table is the dispatcher: `EditorView.vue` asks `matchShortcut` which entry a key press is and runs the action
 * it keeps for that id (a `Record` over the ids of scope `window`, so that an entry without an action does not compile); the help window, the palette and the tooltips
 * read the same table.
 *
 * A chord is written the way it is shown (`['Mod', 'Shift', 'Z']`) and read the same way, so that the text in the help and the key that works cannot differ.
 * Letters are matched by the letter the key makes (`e.key`) and, on a layout that does not make Latin letters (Russian, Greek), by the place of the key
 * (`e.code`): `C` is the key that is `С` on the Russian layout. Signs are matched by what they make.
 */

export type ShortcutGroup = 'general' | 'edit' | 'tools' | 'canvas' | 'simulation'

/**
 * `window`: the page's handler runs it (`matchShortcut`). `card`: keys of a focused card or zone head, handled in `Canvas.vue`; here for the help window.
 * `pointer`: the mouse (and the wheel); here for the help window.
 */
export type ShortcutScope = 'window' | 'card' | 'pointer'

export interface Shortcut {
  /** key of the description (`kb.<id>`) */
  id: string
  group: ShortcutGroup
  scope: ShortcutScope
  /** alternatives shown in the help; each is a chord written with the names below (`Mod` = Ctrl, or ⌘ on macOS) */
  keys: readonly (readonly string[])[]
  /** more chords that work and are not shown (the numeric `+`, `Backspace` for `Del`) */
  also?: readonly (readonly string[])[]
  /** works while a field is typed in or a dialog is open (the palette, save) */
  anywhere?: true
  /** does not work while a button, a link, a tab or a slider has the focus: it takes those keys itself (Space presses a button, the arrows move a slider) */
  skipControls?: true
  /** works while the key is held down (undo, zoom, the arrows of the playback) */
  repeat?: true
}

export const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent || '')

/** How a key is written on this platform. */
export function keyLabel(k: string): string {
  if (k === 'Mod') return isMac ? '⌘' : 'Ctrl'
  if (k === 'Shift') return isMac ? '⇧' : 'Shift'
  if (k === 'Alt') return isMac ? '⌥' : 'Alt'
  return k
}

export const chord = (keys: readonly string[]): string => keys.map(keyLabel).join(isMac ? '' : '+')

export const SHORTCUTS = [
  { id: 'palette', group: 'general', scope: 'window', keys: [['Mod', 'K']], anywhere: true },
  { id: 'shortcuts', group: 'general', scope: 'window', keys: [['?']] },
  { id: 'save', group: 'general', scope: 'window', keys: [['Mod', 'S']], anywhere: true },
  { id: 'escape', group: 'general', scope: 'window', keys: [['Esc']] },
  { id: 'library', group: 'general', scope: 'window', keys: [['/']] },
  { id: 'foldLibrary', group: 'general', scope: 'window', keys: [['[']] },
  { id: 'foldRight', group: 'general', scope: 'window', keys: [[']']] },
  { id: 'export', group: 'general', scope: 'window', keys: [['E']], skipControls: true },
  { id: 'matrix', group: 'general', scope: 'window', keys: [['M']], skipControls: true },
  { id: 'findingNext', group: 'general', scope: 'window', keys: [['N']], skipControls: true, repeat: true },
  { id: 'findingPrev', group: 'general', scope: 'window', keys: [['P']], skipControls: true, repeat: true },
  { id: 'undo', group: 'edit', scope: 'window', keys: [['Mod', 'Z']], repeat: true },
  { id: 'redo', group: 'edit', scope: 'window', keys: [['Mod', 'Shift', 'Z'], ['Mod', 'Y']], repeat: true },
  { id: 'duplicate', group: 'edit', scope: 'window', keys: [['Mod', 'D']] },
  { id: 'delete', group: 'edit', scope: 'window', keys: [['Del']], also: [['Backspace']] },
  { id: 'selectAll', group: 'edit', scope: 'window', keys: [['Mod', 'A']] },
  { id: 'layout', group: 'edit', scope: 'window', keys: [['L']], skipControls: true },
  { id: 'tool.select', group: 'tools', scope: 'window', keys: [['V']] },
  { id: 'tool.connect', group: 'tools', scope: 'window', keys: [['C']] },
  { id: 'tool.zone', group: 'tools', scope: 'window', keys: [['Z']] },
  { id: 'zoomIn', group: 'canvas', scope: 'window', keys: [['+']], also: [['=']], skipControls: true, repeat: true },
  { id: 'zoomOut', group: 'canvas', scope: 'window', keys: [['-']], skipControls: true, repeat: true },
  { id: 'fit', group: 'canvas', scope: 'window', keys: [['0']], skipControls: true },
  { id: 'focus', group: 'canvas', scope: 'window', keys: [['F']], skipControls: true },
  { id: 'canvas.move', group: 'canvas', scope: 'card', keys: [['↑', '↓', '←', '→']] },
  { id: 'canvas.select', group: 'canvas', scope: 'card', keys: [['Enter'], ['Space']] },
  { id: 'canvas.extend', group: 'canvas', scope: 'card', keys: [['Shift', 'Enter']] },
  { id: 'canvas.inspect', group: 'canvas', scope: 'card', keys: [['Enter']] },
  { id: 'multi.click', group: 'canvas', scope: 'pointer', keys: [['Shift', 'click']] },
  { id: 'multi.box', group: 'canvas', scope: 'pointer', keys: [['Shift', 'drag']] },
  { id: 'pan', group: 'canvas', scope: 'pointer', keys: [['drag']] },
  { id: 'zoom', group: 'canvas', scope: 'pointer', keys: [['Mod', 'scroll']] },
  { id: 'play', group: 'simulation', scope: 'window', keys: [['Space']], skipControls: true },
  { id: 'step', group: 'simulation', scope: 'window', keys: [['←'], ['→']], skipControls: true, repeat: true },
] as const satisfies readonly Shortcut[]

export type ShortcutId = (typeof SHORTCUTS)[number]['id']
/** The ids the page's handler has to give an action to: adding an entry of scope `window` without one does not compile. */
export type WindowShortcutId = Extract<(typeof SHORTCUTS)[number], { scope: 'window' }>['id']

export const GROUPS: ShortcutGroup[] = ['general', 'edit', 'tools', 'canvas', 'simulation']

/** `Ctrl+K` for a shortcut id (first alternative): used in tooltips and palette hints. */
export function shortcutText(id: string): string {
  const s = (SHORTCUTS as readonly Shortcut[]).find((x) => x.id === id)
  return s ? chord(s.keys[0]) : ''
}

// --------------------------------------------------------------------------------------------------------------- reading a key press
interface Parsed {
  /** the character the key makes (lower case), or the name of the key (`Escape`, `Delete`, `ArrowLeft`) */
  key: string
  /** the place of the key, for layouts that make other letters (`KeyC`) */
  code?: string
  mod: boolean
  /** `any`: signs (`?`, `+`) are made with Shift on some layouts and without it on others */
  shift: boolean | 'any'
}

const NAMED: Record<string, { key: string; code?: string }> = {
  Esc: { key: 'Escape', code: 'Escape' }, Del: { key: 'Delete', code: 'Delete' }, Backspace: { key: 'Backspace', code: 'Backspace' }, Enter: { key: 'Enter', code: 'Enter' },
  Space: { key: ' ', code: 'Space' }, '←': { key: 'ArrowLeft', code: 'ArrowLeft' }, '→': { key: 'ArrowRight', code: 'ArrowRight' }, '↑': { key: 'ArrowUp', code: 'ArrowUp' }, '↓': { key: 'ArrowDown', code: 'ArrowDown' },
}
/** The brackets are the keys that look like the edges of the panels they fold: by their place, so that the Russian layout (where the keys make `х` and `ъ`) has them too. */
const SIGN_CODE: Record<string, string> = { '[': 'BracketLeft', ']': 'BracketRight' }

/** A chord as written for the help (`['Mod', 'Shift', 'Z']`) → what a key press has to be. Only chords of a key with at most `Mod`, `Shift` and `Alt` before it are keys of the page. */
export function parseChord(keys: readonly string[]): Parsed | null {
  const key = keys[keys.length - 1]
  const modifiers = keys.slice(0, -1)
  if (!key || modifiers.some((m) => m !== 'Mod' && m !== 'Shift')) return null
  const named = NAMED[key]
  const shift = modifiers.includes('Shift')
  if (named) return { ...named, mod: modifiers.includes('Mod'), shift }
  if (key.length !== 1) return null
  if (/[A-Za-z]/.test(key)) return { key: key.toLowerCase(), code: 'Key' + key.toUpperCase(), mod: modifiers.includes('Mod'), shift }
  if (/[0-9]/.test(key)) return { key, code: 'Digit' + key, mod: modifiers.includes('Mod'), shift }
  // a sign: made with or without Shift, by the layout
  return { key, code: SIGN_CODE[key], mod: modifiers.includes('Mod'), shift: shift || 'any' }
}

const PARSED = new Map<string, Parsed[]>()
for (const s of SHORTCUTS as readonly Shortcut[]) {
  if (s.scope !== 'window') continue
  PARSED.set(s.id, [...s.keys, ...(s.also ?? [])].map(parseChord).filter((p): p is Parsed => !!p))
}

/** Does this key press make `p`? */
function matches(e: Pick<KeyboardEvent, 'key' | 'code' | 'ctrlKey' | 'metaKey' | 'shiftKey' | 'altKey'>, p: Parsed): boolean {
  if (e.altKey || (e.ctrlKey || e.metaKey) !== p.mod) return false
  if (p.shift !== 'any' && e.shiftKey !== p.shift) return false
  const made = e.key.length === 1 ? e.key.toLowerCase() : e.key
  if (made === p.key) return true
  // a layout that makes no Latin letter (Russian: `с` on the key of `C`): the place of the key says which it is
  const latin = e.key.length === 1 && /[a-z]/i.test(e.key)
  return !latin && !!p.code && e.code === p.code
}

/** Which shortcut of the page this key press is; `null` when it is none. Whether the shortcut works here (a field has the focus, a dialog is open) is `blocked`. */
export function matchShortcut(e: Pick<KeyboardEvent, 'key' | 'code' | 'ctrlKey' | 'metaKey' | 'shiftKey' | 'altKey'>): WindowShortcutId | null {
  for (const [id, chords] of PARSED) if (chords.some((p) => matches(e, p))) return id as WindowShortcutId
  return null
}

/** Where the key was pressed: what it would be typed into, what takes the key itself, and whether the page is behind a dialog. */
export interface KeyContext {
  /** a field is being typed in */
  typing: boolean
  /** a button, a link, a tab, a slider has the focus */
  control: boolean
  /** a dialog or the palette is open */
  dialog: boolean
}

/** The shortcut does not work in this place (it would be typed, or pressed, or the page is behind a dialog). */
export function blocked(id: ShortcutId, where: KeyContext): boolean {
  const s = (SHORTCUTS as readonly Shortcut[]).find((x) => x.id === id)
  if (!s || s.anywhere) return false
  if (where.typing || where.dialog) return true
  return !!s.skipControls && where.control
}

/** Does the key press repeat while held? */
export function repeats(id: ShortcutId): boolean {
  return !!(SHORTCUTS as readonly Shortcut[]).find((x) => x.id === id)?.repeat
}
