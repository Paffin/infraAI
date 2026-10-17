/**
 * Where keyboard focus goes when the control that had it is gone (a dialog opened from a card that was deleted) or when the page itself changes
 * (a project opened from the list): the heading that names the page. A heading is not focusable by itself, so it gets `tabindex="-1"`: focusable by a
 * script, never in the Tab order, and no outline (base.css).
 */
export function pageHeading(): HTMLElement | null {
  return document.querySelector<HTMLElement>('#app h1')
}

/** Moves the focus to the heading of the page (or, lacking one, to its main landmark). Returns whether the focus is there now. */
export function focusPage(): boolean {
  const heading = pageHeading()
  // a heading with no words (the editor's, until the project is loaded) is nothing to be read: the main area then
  const target = heading?.textContent?.trim() ? heading : document.querySelector<HTMLElement>('#app main, #app [role="main"]') ?? heading
  if (!target) return false
  if (!target.hasAttribute('tabindex')) target.setAttribute('tabindex', '-1')
  target.focus({ preventScroll: true })
  return document.activeElement === target
}

/** Nothing in particular has the focus: the body, or no element at all. */
export function focusIsFree(): boolean {
  const a = document.activeElement
  return !a || a === document.body || a === document.documentElement
}
