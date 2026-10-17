import { afterEach, describe, expect, it } from 'vitest'
import { KEEP_LIVE, lockInert, lockOthers } from '../src/lib/inert'

// Real DOM (jsdom). `inert` is asserted as the ATTRIBUTE: jsdom has no `inert` property, and the attribute is what the browser acts on.

afterEach(() => { document.body.innerHTML = '' })

function el(tag = 'div', attrs: Record<string, string> = {}): HTMLElement {
  const e = document.createElement(tag)
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v)
  document.body.appendChild(e)
  return e
}

describe('lockInert', () => {
  it('makes the element inert and gives it back', () => {
    const a = el()
    const unlock = lockInert(a)
    expect(a.hasAttribute('inert')).toBe(true)
    unlock()
    expect(a.hasAttribute('inert')).toBe(false)
  })

  it('is counted: two locks, one released, still inert; both released, free', () => {
    const a = el()
    const first = lockInert(a)
    const second = lockInert(a)
    first()
    expect(a.hasAttribute('inert')).toBe(true)
    second()
    expect(a.hasAttribute('inert')).toBe(false)
  })

  it('releases a lock once: calling it again does not take the other lock away', () => {
    const a = el()
    const first = lockInert(a)
    const second = lockInert(a)
    first()
    first()
    expect(a.hasAttribute('inert')).toBe(true)
    second()
    expect(a.hasAttribute('inert')).toBe(false)
  })

  it('does not take away an inert that the element had before', () => {
    const a = el('div', { inert: '' })
    const unlock = lockInert(a)
    unlock()
    expect(a.hasAttribute('inert')).toBe(true)
  })

  it('locks are released in any order', () => {
    const a = el()
    const outer = lockInert(a)
    const inner = lockInert(a)
    outer()
    expect(a.hasAttribute('inert')).toBe(true)
    inner()
    expect(a.hasAttribute('inert')).toBe(false)
  })
})

describe('lockOthers', () => {
  it('locks the siblings of the dialog, not the dialog, not scripts, not what stays live', () => {
    const app = el('div', { id: 'app' })
    const script = el('script')
    const toasts = el('div', { [KEEP_LIVE]: '' })
    const below = el('div', { class: 'modal-backdrop' })
    const dialog = el('div', { class: 'modal-backdrop' })
    const unlock = lockOthers(dialog)
    expect(app.hasAttribute('inert')).toBe(true)
    expect(below.hasAttribute('inert')).toBe(true)
    expect(dialog.hasAttribute('inert')).toBe(false)
    expect(script.hasAttribute('inert')).toBe(false)
    expect(toasts.hasAttribute('inert')).toBe(false)
    unlock()
    for (const e of [app, script, toasts, below, dialog]) expect(e.hasAttribute('inert')).toBe(false)
  })

  it('stacked dialogs: the page stays inert until the last one closes', () => {
    const app = el('div', { id: 'app' })
    const first = el('div')
    const unlockFirst = lockOthers(first)
    const second = el('div')
    const unlockSecond = lockOthers(second)
    expect(first.hasAttribute('inert')).toBe(true)
    unlockSecond()
    expect(first.hasAttribute('inert')).toBe(false)
    expect(app.hasAttribute('inert')).toBe(true)
    unlockFirst()
    expect(app.hasAttribute('inert')).toBe(false)
  })

  it('a dialog deeper than a child of the body keeps its way free: the siblings on the way down are locked, its ancestors are not', () => {
    const wrap = el('div')
    const beside = document.createElement('p')
    const dialog = document.createElement('div')
    wrap.append(beside, dialog)
    const other = el('div')
    const unlock = lockOthers(dialog)
    expect(wrap.hasAttribute('inert')).toBe(false)
    expect(beside.hasAttribute('inert')).toBe(true)
    expect(other.hasAttribute('inert')).toBe(true)
    unlock()
    expect(beside.hasAttribute('inert')).toBe(false)
    expect(other.hasAttribute('inert')).toBe(false)
  })

  it('the tour and a dialog on the same page: neither takes the lock of the other away', () => {
    const app = el('div', { id: 'app' })
    const dialog = el('div')
    const unlockDialog = lockOthers(dialog)
    const unlockTour = lockInert(app) // what TourHost does
    unlockDialog()
    expect(app.hasAttribute('inert')).toBe(true)
    unlockTour()
    expect(app.hasAttribute('inert')).toBe(false)
  })
})
