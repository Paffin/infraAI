import { onBeforeUnmount, onMounted, type Ref } from 'vue'
import { lockOthers } from './inert'
import { focusPage } from './pageFocus'

/**
 * A dialog takes the keyboard, holds it and gives it back.
 *
 *  - TAKES: when it opens, focus moves into it (the field, then the first control of the body, then of the footer, then the close button, then the dialog itself).
 *  - HOLDS: everything else on the page is `inert` (not reachable by Tab, the mouse or a screen reader) and Tab / Shift+Tab wrap at the ends, so the keyboard
 *    never leaves, whatever the browser does about `inert`.
 *  - RETURNS: when it closes, focus goes back to the control that had it when the dialog opened; if that control is gone, to the dialog underneath
 *    or to the heading of the page.
 *
 * Scopes stack (a confirm over the versions window, the palette over a dialog). Only the top one hears Escape and Tab; the ones below stay inert. The
 * stack is shared by every dialog on purpose: two listeners of their own would both hear one Escape and close both dialogs at once.
 */
export interface FocusScopeOptions {
  /** Escape was pressed while this scope is the top one. Without it the key is still kept from the page below. */
  onEscape?: () => void
  /** The dialog itself (focus goes there when nothing inside takes it). Default: the root. */
  container?: () => HTMLElement | null
  /** Where the focus goes first, when `[autofocus]` and the regions give nothing better. */
  initial?: () => HTMLElement | null | undefined
  /** Selectors searched in order for the first control to focus (inside the root), after an `[autofocus]` element. */
  regions?: string[]
  /** `cycle` (default): Tab goes round the controls of the dialog. `hold`: the dialog is one control (a combobox): Tab keeps the focus where it is. */
  tab?: 'cycle' | 'hold'
}

export interface FocusScope {
  /** Take the keyboard: lock the page, join the stack, move the focus in. Does nothing if already active. */
  activate(): void
  /** Give the keyboard back: unlock the page, leave the stack, put the focus where it was. Does nothing if not active. */
  release(): void
  /** The control that had the focus when the scope was made. */
  readonly opener: HTMLElement | null
  readonly active: boolean
}

// ---------------------------------------------------------------------------------------------------------------- what can be focused
const FOCUSABLE = 'a[href], area[href], button, input:not([type="hidden"]), select, textarea, summary, iframe, audio[controls], video[controls], [contenteditable]:not([contenteditable="false"]), [tabindex]'

/** Not drawn: `display: none` (or `hidden`) on it or above it up to the root, or `inert` above it. Works without layout (jsdom has none). */
function hiddenWithin(el: Element, root: Element, cache: Map<Element, boolean>): boolean {
  let hidden = false
  const walk: Element[] = []
  for (let n: Element | null = el; n; n = n === root ? null : n.parentElement) {
    const known = cache.get(n)
    if (known !== undefined) { hidden = known; break }
    walk.push(n)
    if (n.hasAttribute('hidden') || n.hasAttribute('inert') || getComputedStyle(n).display === 'none') { hidden = true; break }
  }
  for (const n of walk) cache.set(n, hidden)
  return hidden
}

function usable(el: HTMLElement, root: Element, cache: Map<Element, boolean>): boolean {
  if (el.matches(':disabled') || hiddenWithin(el, root, cache)) return false
  const v = getComputedStyle(el).visibility
  return v !== 'hidden' && v !== 'collapse'
}

/** Controls in the Tab order inside `root`, in document order. A group of radio buttons is one stop: the checked one (the first, when none is). */
export function tabbables(root: Element): HTMLElement[] {
  const cache = new Map<Element, boolean>()
  const stops = Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE)).filter((el) => {
    const ti = el.getAttribute('tabindex')
    return !(ti !== null && Number.parseInt(ti, 10) < 0) && usable(el, root, cache)
  })
  const groups = new Map<string, HTMLInputElement[]>()
  for (const el of stops) {
    if (el instanceof HTMLInputElement && el.type === 'radio' && el.name) {
      const key = `${el.form ? 'form' : 'root'}:${el.name}`
      groups.set(key, [...(groups.get(key) ?? []), el])
    }
  }
  const skip = new Set<HTMLElement>()
  for (const radios of groups.values()) {
    const keep = radios.find((r) => r.checked) ?? radios[0]
    for (const r of radios) if (r !== keep) skip.add(r)
  }
  return stops.filter((el) => !skip.has(el))
}

function activeElement(): HTMLElement | null {
  const a = document.activeElement
  return a && a !== document.body && a !== document.documentElement ? (a as HTMLElement) : null
}

// ---------------------------------------------------------------------------------------------------------------- the stack
interface Entry { scope: FocusScope; root: () => HTMLElement | null; dialog: () => HTMLElement | null; opts: FocusScopeOptions; focusInside: () => void }
const stack: Entry[] = []
let listening = false

function onKey(e: KeyboardEvent) {
  const top = stack[stack.length - 1]
  if (!top || e.isComposing) return
  if (e.key === 'Escape') {
    // the page below never hears it, and neither does the next scope: one Escape closes one thing
    e.stopPropagation()
    top.opts.onEscape?.()
  } else if (e.key === 'Tab') {
    keepTab(top, e)
  }
}

function keepTab(top: Entry, e: KeyboardEvent) {
  if (top.opts.tab === 'hold') { e.preventDefault(); return }
  const root = top.root()
  if (!root) return
  const list = tabbables(root)
  if (!list.length) { e.preventDefault(); top.focusInside(); return }
  const a = document.activeElement as HTMLElement | null
  const inside = !!a && a !== document.body && root.contains(a)
  const at = a ? list.indexOf(a) : -1
  if (e.shiftKey) {
    if (at === 0 || !inside || a === top.dialog()) { e.preventDefault(); list[list.length - 1].focus() }
  } else if (at === list.length - 1 || !inside) {
    e.preventDefault()
    list[0].focus()
  }
}

function listen(on: boolean) {
  if (on === listening) return
  listening = on
  // capture on the window: before the page's own handlers; the tour (registered earlier) still hears its keys first
  if (on) window.addEventListener('keydown', onKey, true)
  else window.removeEventListener('keydown', onKey, true)
}

/** Is `scope` the one on top, the one that owns the keyboard? */
export function isTopScope(scope: FocusScope): boolean {
  return stack[stack.length - 1]?.scope === scope
}

/** How many scopes are open (tests, and the code that must know whether a dialog is on screen). */
export function openScopes(): number { return stack.length }

// ---------------------------------------------------------------------------------------------------------------- one scope
export function createFocusScope(root: () => HTMLElement | null, opts: FocusScopeOptions = {}): FocusScope {
  // remembered NOW, before the dialog is drawn: a click on a menu item or a palette command removes the very control that had the focus
  const opener = activeElement()
  let unlock: (() => void) | null = null
  let active = false
  const dialog = () => opts.container?.() ?? root()

  function focusInside() {
    const el = root()
    if (!el) return
    const cache = new Map<Element, boolean>()
    const pick = (): HTMLElement | null | undefined => {
      const given = opts.initial?.()
      if (given) return given
      const auto = Array.from(el.querySelectorAll<HTMLElement>('[autofocus]')).find((x) => usable(x, el, cache))
      if (auto) return auto
      for (const sel of opts.regions ?? []) {
        const region = el.querySelector(sel)
        const first = region ? tabbables(region)[0] : undefined
        if (first) return first
      }
      return tabbables(el)[0]
    }
    let target = pick()
    if (!target) {
      // nothing to press: the dialog itself takes the focus, so Tab and Escape have a place to start from
      target = dialog()
      if (target && !target.hasAttribute('tabindex')) target.setAttribute('tabindex', '-1')
    }
    target?.focus({ preventScroll: true })
  }

  const entry: Entry = { scope: undefined as unknown as FocusScope, root, dialog, opts, focusInside }

  function restore() {
    if (!opener) { // nothing had the focus before: nothing needs it back, and nothing of the dialog keeps it
      const a = document.activeElement
      if (a && a !== document.body && root()?.contains(a)) (a as HTMLElement).blur()
      return
    }
    if (opener.isConnected) {
      opener.focus({ preventScroll: true })
      if (document.activeElement === opener) return
    }
    // the control is gone (a deleted card, a closed menu) or cannot take the focus: the dialog underneath, or the heading of the page
    const below = stack[stack.length - 1]
    if (below) below.focusInside()
    else focusPage()
  }

  const scope: FocusScope = {
    get opener() { return opener },
    get active() { return active },
    activate() {
      const el = root()
      if (active || !el) return
      active = true
      unlock = lockOthers(el)
      stack.push(entry)
      listen(true)
      focusInside()
    },
    release() {
      if (!active) return
      active = false
      const el = root()
      const wasTop = stack[stack.length - 1] === entry
      const a = document.activeElement
      const heldFocus = !a || a === document.body || !!el?.contains(a)
      const at = stack.indexOf(entry)
      if (at >= 0) stack.splice(at, 1)
      if (!stack.length) listen(false)
      // the page must be reachable again BEFORE the focus goes back to it: focus() on a control inside an inert subtree does nothing
      unlock?.()
      unlock = null
      // a scope that is not on top does not own the focus (the one above it does); one that sent the focus elsewhere on purpose keeps that
      if (wasTop && heldFocus) restore()
    },
  }
  entry.scope = scope
  return scope
}

/** The composable: the scope lives as long as the component that owns `root` (the element teleported to the body). */
export function useFocusScope(root: Ref<HTMLElement | null>, opts: FocusScopeOptions = {}): FocusScope {
  const scope = createFocusScope(() => root.value, opts)
  onMounted(() => scope.activate())
  onBeforeUnmount(() => scope.release())
  return scope
}
