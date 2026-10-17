import { onBeforeUnmount, onMounted, type Ref } from 'vue'

/**
 * A list of hundreds of buttons (the library of products: 8 headers of categories, 95 headers of functions, 291 tiles, 20 reference architectures) as ONE
 * stop of the Tab key: exactly one of the buttons has `tabindex="0"`, the others `-1`. ArrowDown / ArrowUp go to the next / previous button in the order of the
 * page, Home / End to the first / last, ArrowRight / ArrowLeft open / close a button that folds something (`aria-expanded`). Enter and Space are the buttons' own.
 *
 * It reads the page, it keeps nothing of its own: the buttons are looked for at the moment a key is pressed and after the page changes (a group is folded, a search
 * narrows the list, the ranking changes), so no state can say "tabindex 0" about a button that is gone, and the list is never left without a stop.
 */
export function useRoving(container: Ref<HTMLElement | null>, selector = 'button') {
  let stop: HTMLElement | null = null
  let watcher: MutationObserver | undefined

  /** The buttons that can be focused now, in the order of the page. */
  function items(root: HTMLElement): HTMLElement[] {
    return Array.from(root.querySelectorAll<HTMLElement>(selector)).filter((el) => !el.hasAttribute('disabled') && !el.hidden)
  }
  const want = (el: HTMLElement, value: '0' | '-1') => { if (el.getAttribute('tabindex') !== value) el.setAttribute('tabindex', value) }

  /** Exactly one stop: the one the list had if it is still there, else the first. */
  function ensure() {
    const root = container.value
    if (!root) return
    const list = items(root)
    if (!list.length) return
    const keep = stop && list.includes(stop) ? stop : list.find((el) => el.getAttribute('tabindex') === '0') ?? list[0]
    stop = keep
    for (const el of list) want(el, el === keep ? '0' : '-1')
  }

  function onFocusin(e: FocusEvent) {
    const root = container.value
    const el = e.target instanceof HTMLElement ? e.target.closest<HTMLElement>(selector) : null
    if (!root || !el || !root.contains(el) || el.hasAttribute('disabled')) return
    stop = el
    ensure()
  }

  function onKeydown(e: KeyboardEvent) {
    const root = container.value
    if (!root || e.altKey || e.ctrlKey || e.metaKey || e.shiftKey || e.isComposing) return
    const from = e.target instanceof HTMLElement ? e.target.closest<HTMLElement>(selector) : null
    if (!from) return
    const list = items(root)
    const at = list.indexOf(from)
    if (at < 0) return
    let to: HTMLElement | undefined
    if (e.key === 'ArrowDown') to = list[at + 1]
    else if (e.key === 'ArrowUp') to = list[at - 1]
    else if (e.key === 'Home') to = list[0]
    else if (e.key === 'End') to = list[list.length - 1]
    else if ((e.key === 'ArrowRight' && from.getAttribute('aria-expanded') === 'false') || (e.key === 'ArrowLeft' && from.getAttribute('aria-expanded') === 'true')) {
      e.preventDefault()
      from.click()
      return
    } else return
    e.preventDefault()
    to?.focus()
  }

  onMounted(() => {
    const root = container.value
    if (!root) return
    root.addEventListener('focusin', onFocusin)
    root.addEventListener('keydown', onKeydown)
    ensure()
    // buttons come and go with the folds, the search and the ranking; childList only: giving a button its tabindex is not a change this has to hear
    watcher = new MutationObserver(ensure)
    watcher.observe(root, { childList: true, subtree: true })
  })
  onBeforeUnmount(() => {
    watcher?.disconnect()
    container.value?.removeEventListener('focusin', onFocusin)
    container.value?.removeEventListener('keydown', onKeydown)
  })

  return { ensure }
}
