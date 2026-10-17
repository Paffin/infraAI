/**
 * `inert` on an element takes it (and everything inside) out of the page for the mouse, Tab and a screen reader. Several things want it at once:
 * a dialog over the page, a second dialog over the first, the tour over a dialog. A bare setAttribute / removeAttribute lets the one that closes first
 * give the page back to everybody, so every lock is counted per element and the attribute goes only when the last lock is released.
 *
 * It works on the ATTRIBUTE: the `inert` property does not exist everywhere (jsdom has none) and the attribute is what a test can read.
 */
const locks = new WeakMap<Element, number>()
/** Elements that were inert before the first lock: the lock must not take that away from their author. */
const own = new WeakSet<Element>()

/** Make `el` inert; the returned function releases this one lock (calling it again does nothing). */
export function lockInert(el: Element): () => void {
  const n = locks.get(el) ?? 0
  if (n === 0) {
    if (el.hasAttribute('inert')) own.add(el)
    else el.setAttribute('inert', '')
  }
  locks.set(el, n + 1)
  let released = false
  return () => {
    if (released) return
    released = true
    const left = (locks.get(el) ?? 1) - 1
    if (left > 0) { locks.set(el, left); return }
    locks.delete(el)
    if (!own.delete(el)) el.removeAttribute('inert')
  }
}

/** Elements that are not part of the page the user sees. */
const NOT_CONTENT = new Set(['SCRIPT', 'STYLE', 'LINK', 'META', 'TEMPLATE', 'NOSCRIPT', 'TITLE'])

/** Attribute of a child of the body that stays alive under a dialog (the toasts: an error raised by the dialog must be read and clicked). */
export const KEEP_LIVE = 'data-keep-live'

/**
 * Make everything in `parent` (the body) inert except `keep`, what contains it and what stays live: a dialog is teleported to the body, so its siblings
 * are the page (`#app`), the dialogs below it and the tour. Where `keep` is deeper than a child of `parent`, the siblings on the way down are locked
 * instead, so it still stays reachable. Returns the function that gives them all back.
 */
export function lockOthers(keep: Element, parent: Element = document.body): () => void {
  const undo: Array<() => void> = []
  let node: Element | null = keep
  while (node && node !== parent && node.parentElement) {
    for (const sibling of Array.from(node.parentElement.children)) {
      if (sibling === node || NOT_CONTENT.has(sibling.tagName) || sibling.hasAttribute(KEEP_LIVE)) continue
      undo.push(lockInert(sibling))
    }
    node = node.parentElement
  }
  return () => { for (const u of undo) u() }
}
