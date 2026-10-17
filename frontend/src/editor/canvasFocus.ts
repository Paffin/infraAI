/**
 * The DOM side of the keyboard walk over the diagram (the geometry is `lib/canvasNav.ts`).
 *
 * Every card and every zone head carries `data-kbd` (`node` / `zone`) and `data-id`. The whole diagram is ONE stop of the Tab key (roving tabindex): exactly one of
 * them has `tabindex="0"`, the others `-1`; the arrow keys move between them and the focus moves the stop along. Nothing is kept in reactive state, so a
 * diagram of hundreds of cards costs nothing more than one with ten, and everything here reads the DOM at the moment a key is pressed.
 */
import { nearestTo, nextInDirection, readingOrder, type Dir, type NavBox } from '@/lib/canvasNav'

export const ITEM = '[data-kbd]'
const STOP = `${ITEM}[tabindex="0"]`

/** `node:n1`, `zone:z1`: the key of an item, unique on the diagram. */
export const keyOf = (el: HTMLElement) => `${el.dataset.kbd}:${el.dataset.id}`

export const canvasEl = (): HTMLElement | null => document.querySelector<HTMLElement>('.canvas')

/** The items on the diagram, in document order. */
export const itemsOf = (root: ParentNode): HTMLElement[] => Array.from(root.querySelectorAll<HTMLElement>(ITEM))

function boxesOf(els: HTMLElement[]): { boxes: NavBox[]; byKey: Map<string, HTMLElement> } {
  const boxes: NavBox[] = []
  const byKey = new Map<string, HTMLElement>()
  for (const el of els) {
    const r = el.getBoundingClientRect()
    if (!r.width && !r.height) continue // not drawn
    const id = keyOf(el)
    byKey.set(id, el)
    boxes.push({ id, x: r.left, y: r.top, w: r.width, h: r.height })
  }
  return { boxes, byKey }
}

/** Make `el` the one stop of the Tab key on the diagram. */
export function setStop(root: ParentNode, el: HTMLElement) {
  for (const other of Array.from(root.querySelectorAll<HTMLElement>(STOP))) if (other !== el) other.setAttribute('tabindex', '-1')
  if (el.getAttribute('tabindex') !== '0') el.setAttribute('tabindex', '0')
}

/**
 * Exactly one stop on the diagram, whatever happened to its cards (a card was deleted, a template replaced them all, a project opened): the one that has it
 * and is still there; else `prefer`; else the selected card; else the first card in reading order. `null` when there is nothing to stop at.
 */
export function ensureStop(root: ParentNode, prefer?: HTMLElement | null): HTMLElement | null {
  const stops = Array.from(root.querySelectorAll<HTMLElement>(STOP))
  if (stops.length === 1 && !prefer) return stops[0]
  const pick = prefer && prefer.isConnected ? prefer : stops[0] ?? root.querySelector<HTMLElement>(`${ITEM}.selected`) ?? edgeItem(root, 'first')
  if (pick) setStop(root, pick)
  return pick
}

/** Where a skip link or a "back to the diagram" lands: the stop of the diagram; the canvas itself when it has no card yet (an empty project, one still loading). */
export function focusDiagram(): boolean {
  const canvas = canvasEl()
  if (!canvas) return false
  const target = ensureStop(canvas) ?? canvas
  target.focus({ preventScroll: true })
  return document.activeElement === target
}

/** The item for a selected card or zone (a link has none). */
export function itemFor(root: ParentNode, sel: { type: 'node' | 'edge' | 'zone'; id: string } | null): HTMLElement | null {
  if (!sel || sel.type === 'edge') return null
  return root.querySelector<HTMLElement>(`${ITEM}[data-kbd="${sel.type === 'node' ? 'node' : 'zone'}"][data-id="${CSS.escape(sel.id)}"]`)
}

/** The item an arrow key goes to from `from`; `null` at the edge of the diagram. */
export function neighbour(root: ParentNode, from: HTMLElement, dir: Dir): HTMLElement | null {
  const { boxes, byKey } = boxesOf(itemsOf(root))
  const key = nextInDirection(boxes, keyOf(from), dir)
  return key ? byKey.get(key) ?? null : null
}

/** The first or the last card in reading order (a zone head when there are no cards): Home / End, and where Tab enters a diagram nobody has been on yet. */
export function edgeItem(root: ParentNode, which: 'first' | 'last'): HTMLElement | null {
  const all = itemsOf(root)
  const cards = all.filter((el) => el.dataset.kbd === 'node')
  const { boxes, byKey } = boxesOf(cards.length ? cards : all)
  const order = readingOrder(boxes)
  const pick = which === 'first' ? order[0] : order[order.length - 1]
  return pick ? byKey.get(pick.id) ?? null : null
}

/** Where the focus goes when `from` is about to be deleted along with `doomed`: the closest item that stays. */
export function successor(root: ParentNode, from: HTMLElement, doomed: ReadonlySet<string>): HTMLElement | null {
  const { boxes, byKey } = boxesOf(itemsOf(root))
  const key = nearestTo(boxes, keyOf(from), doomed)
  return key ? byKey.get(key) ?? null : null
}

/** Is the focus on this element because of the keyboard (and not a click)? Only that moves the view. */
export function byKeyboard(el: HTMLElement): boolean {
  try { return el.matches(':focus-visible') } catch { return true }
}
