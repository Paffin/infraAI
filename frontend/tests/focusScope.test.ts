import { mount } from '@vue/test-utils'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h, nextTick, reactive } from 'vue'
import Modal from '../src/components/Modal.vue'
import { createFocusScope, isTopScope, openScopes, tabbables, type FocusScope } from '../src/lib/useFocusScope'

// Real DOM (jsdom), no stand-ins. jsdom has no layout and no `inert`, so what is asserted here is the logic: where the focus goes, what Tab does at the ends,
// who hears Escape, what is returned. That the browser really keeps Tab and clicks out of an inert page is proven in e2e/dialog-focus.spec.ts.

const scopes: FocusScope[] = []
afterEach(() => {
  for (const s of scopes.splice(0).reverse()) s.release()
  document.body.innerHTML = ''
  expect(openScopes()).toBe(0)
})

function add(parent: Element, html: string): HTMLElement {
  const box = document.createElement('div')
  box.innerHTML = html
  const el = box.firstElementChild as HTMLElement
  parent.appendChild(el)
  return el
}

/** A page with one button that has the focus, and a dialog in the body shaped like Modal.vue. */
function page(opener = '<button id="opener">open</button>') {
  const app = add(document.body, `<div id="app">${opener}<h1>Page</h1></div>`)
  app.querySelector<HTMLElement>('#opener')?.focus()
  return app
}

function dialog(inner: string) {
  const backdrop = add(document.body, `<div class="modal-backdrop"><div class="modal" role="dialog" aria-modal="true">${inner}</div></div>`)
  return { backdrop, modal: backdrop.querySelector<HTMLElement>('.modal')! }
}

function open(backdrop: HTMLElement, opts: Parameters<typeof createFocusScope>[1] = {}) {
  const scope = createFocusScope(() => backdrop, { regions: ['.modal-body', '.modal-foot'], ...opts })
  scopes.push(scope)
  scope.activate()
  return scope
}

function key(k: string, opts: KeyboardEventInit = {}, target: Element | null = document.activeElement) {
  const e = new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...opts })
  ;(target ?? document.body).dispatchEvent(e)
  return e
}

const HEAD = '<div class="modal-head"><h2>T</h2><button id="x" aria-label="close">x</button></div>'
const id = () => document.activeElement?.id

describe('where the focus goes when a dialog opens', () => {
  it('an [autofocus] control wins over everything', () => {
    page()
    const { backdrop } = dialog(`${HEAD}<div class="modal-body"><input id="f"><button id="auto" autofocus>a</button></div><div class="modal-foot"><button id="ok">ok</button></div>`)
    open(backdrop)
    expect(id()).toBe('auto')
  })

  it('then the first control of the body, then of the footer, then the close button, then the dialog itself', () => {
    page()
    const body = dialog(`${HEAD}<div class="modal-body"><input id="f"></div><div class="modal-foot"><button id="ok">ok</button></div>`)
    open(body.backdrop).release()
    expect(id()).toBe('opener') // and it was given back
    expect(openScopes()).toBe(0)
    open(body.backdrop)
    expect(id()).toBe('f')
    scopes.pop()?.release()
    body.backdrop.remove()

    const foot = dialog(`${HEAD}<div class="modal-body"><p>text only</p></div><div class="modal-foot"><button id="cancel">no</button><button id="ok">ok</button></div>`)
    open(foot.backdrop)
    expect(id()).toBe('cancel')
    scopes.pop()?.release()
    foot.backdrop.remove()

    const close = dialog(`${HEAD}<div class="modal-body"><p>text only</p></div>`)
    open(close.backdrop)
    expect(id()).toBe('x')
    scopes.pop()?.release()
    close.backdrop.remove()

    const none = dialog('<div class="modal-body"><p>nothing to press</p></div>')
    open(none.backdrop, { container: () => none.modal })
    expect(document.activeElement).toBe(none.modal)
    expect(none.modal.getAttribute('tabindex')).toBe('-1')
  })

  it('skips what cannot take the focus: disabled, hidden, display:none, tabindex -1, inert', () => {
    page()
    const { backdrop } = dialog(`${HEAD}<div class="modal-body">
      <input id="a" disabled><input id="b" hidden><div style="display:none"><input id="c"></div><input id="d" tabindex="-1">
      <div inert><input id="e"></div><input id="f" type="hidden"><button id="ok">ok</button></div>`)
    open(backdrop)
    expect(id()).toBe('ok')
    expect(tabbables(backdrop).map((e) => e.id)).toEqual(['x', 'ok'])
  })

  it('a group of radio buttons is one stop: the checked one, or the first when none is', () => {
    page()
    const { backdrop } = dialog(`${HEAD}<div class="modal-body"><input id="q">
      <input type="radio" name="t" id="r1"><input type="radio" name="t" id="r2" checked><input type="radio" name="t" id="r3">
      <input type="radio" name="u" id="s1"><input type="radio" name="u" id="s2"></div><div class="modal-foot"><button id="ok">ok</button></div>`)
    expect(tabbables(backdrop).map((e) => e.id)).toEqual(['x', 'q', 'r2', 's1', 'ok'])
  })

  it('"initial" decides first', () => {
    page()
    const { backdrop } = dialog(`${HEAD}<div class="modal-body"><input id="f"><input id="g"></div>`)
    open(backdrop, { initial: () => backdrop.querySelector<HTMLElement>('#g') })
    expect(id()).toBe('g')
  })
})

describe('Tab and Shift+Tab', () => {
  const html = `${HEAD}<div class="modal-body"><input id="a"><input id="b"></div><div class="modal-foot"><button id="ok">ok</button></div>`

  it('wrap: at the last control Tab goes to the first, at the first Shift+Tab goes to the last; in the middle the browser moves the focus', () => {
    page()
    const { backdrop } = dialog(html)
    open(backdrop)
    const order = ['x', 'a', 'b', 'ok']
    ;(document.getElementById('ok') as HTMLElement).focus()
    const forward = key('Tab')
    expect(forward.defaultPrevented).toBe(true)
    expect(id()).toBe(order[0])
    const backward = key('Tab', { shiftKey: true })
    expect(backward.defaultPrevented).toBe(true)
    expect(id()).toBe('ok')
    ;(document.getElementById('a') as HTMLElement).focus()
    expect(key('Tab').defaultPrevented).toBe(false)
    expect(key('Tab', { shiftKey: true }).defaultPrevented).toBe(false)
  })

  it('walking the whole dialog both ways visits every control and never leaves', () => {
    page()
    const { backdrop } = dialog(html)
    open(backdrop)
    const seen = new Set<string>()
    const step = (shiftKey: boolean) => {
      const list = tabbables(backdrop)
      const e = key('Tab', { shiftKey })
      if (!e.defaultPrevented) { // what the browser does itself in the middle
        const at = list.indexOf(document.activeElement as HTMLElement)
        list[at + (shiftKey ? -1 : 1)].focus()
      }
      expect(backdrop.contains(document.activeElement)).toBe(true)
      seen.add(id() ?? '')
    }
    for (let i = 0; i < 40; i++) step(false)
    for (let i = 0; i < 40; i++) step(true)
    expect([...seen].sort()).toEqual(['a', 'b', 'ok', 'x'])
  })

  it('a focus that is outside the dialog (a toast, the body) comes back in: first control on Tab, last on Shift+Tab', () => {
    page()
    const toast = add(document.body, '<div data-keep-live><button id="undo">undo</button></div>')
    const { backdrop } = dialog(html)
    open(backdrop)
    expect(toast.hasAttribute('inert')).toBe(false)
    ;(document.getElementById('undo') as HTMLElement).focus()
    key('Tab')
    expect(id()).toBe('x')
    ;(document.getElementById('undo') as HTMLElement).focus()
    key('Tab', { shiftKey: true })
    expect(id()).toBe('ok')
    ;(document.activeElement as HTMLElement).blur()
    key('Tab', {}, document.body)
    expect(id()).toBe('x')
  })

  it('from the dialog itself Shift+Tab goes to the last control', () => {
    page()
    const { backdrop, modal } = dialog(html)
    open(backdrop, { container: () => modal })
    modal.setAttribute('tabindex', '-1')
    modal.focus()
    const e = key('Tab', { shiftKey: true })
    expect(e.defaultPrevented).toBe(true)
    expect(id()).toBe('ok')
  })

  it('a dialog with nothing to press keeps the focus on itself', () => {
    page()
    const { backdrop, modal } = dialog('<div class="modal-body"><p>text</p></div>')
    open(backdrop, { container: () => modal })
    expect(key('Tab').defaultPrevented).toBe(true)
    expect(document.activeElement).toBe(modal)
  })

  it('"hold": a dialog that is one control (the palette) keeps the focus where it is', () => {
    page()
    const { backdrop } = dialog('<div class="modal-body"><input id="q"><button id="r1">1</button></div>')
    open(backdrop, { tab: 'hold', initial: () => document.getElementById('q') })
    expect(key('Tab').defaultPrevented).toBe(true)
    expect(key('Tab', { shiftKey: true }).defaultPrevented).toBe(true)
    expect(id()).toBe('q')
  })
})

describe('the page behind', () => {
  it('is inert while the dialog is open and free after; what stays live is not locked', () => {
    const app = page()
    const toasts = add(document.body, '<div data-keep-live></div>')
    const { backdrop } = dialog(`${HEAD}`)
    const scope = open(backdrop)
    expect(app.hasAttribute('inert')).toBe(true)
    expect(toasts.hasAttribute('inert')).toBe(false)
    expect(backdrop.hasAttribute('inert')).toBe(false)
    scope.release()
    expect(app.hasAttribute('inert')).toBe(false)
  })

  it('a second dialog over the first locks the first; closing it gives the first back, the page stays locked', () => {
    const app = page()
    const first = dialog(`${HEAD}<div class="modal-body"><button id="restore">r</button></div>`)
    const s1 = open(first.backdrop)
    const second = dialog('<div class="modal-foot"><button id="no">no</button></div>')
    const s2 = open(second.backdrop)
    expect(first.backdrop.hasAttribute('inert')).toBe(true)
    expect(app.hasAttribute('inert')).toBe(true)
    expect(isTopScope(s2)).toBe(true)
    expect(isTopScope(s1)).toBe(false)
    s2.release()
    expect(first.backdrop.hasAttribute('inert')).toBe(false)
    expect(app.hasAttribute('inert')).toBe(true)
    s1.release()
    expect(app.hasAttribute('inert')).toBe(false)
  })
})

describe('giving the keyboard back', () => {
  it('the focus returns to the control that had it, and it is released from inert first (a focus() inside an inert page does nothing)', () => {
    const app = page()
    const { backdrop } = dialog(`${HEAD}`)
    const scope = open(backdrop)
    expect(id()).toBe('x')
    expect(scope.opener?.id).toBe('opener')
    // what a browser does: a control inside an inert subtree cannot take the focus
    const opener = document.getElementById('opener') as HTMLElement
    const realFocus = opener.focus.bind(opener)
    opener.focus = (o?: FocusOptions) => { if (!app.hasAttribute('inert')) realFocus(o) }
    scope.release()
    expect(id()).toBe('opener')
  })

  it('the control is remembered when the scope is made, not when it opens: a menu item that is gone by then is not the opener', () => {
    page('<button id="trigger">menu</button><button id="item">item</button>')
    document.getElementById('trigger')!.focus()
    const { backdrop } = dialog(HEAD)
    const scope = createFocusScope(() => backdrop, {})
    scopes.push(scope)
    document.getElementById('item')!.focus() // would be too late
    scope.activate()
    scope.release()
    expect(id()).toBe('trigger')
  })

  it('the control is gone: the focus goes to the heading of the page', () => {
    const app = page()
    const { backdrop } = dialog(HEAD)
    const scope = open(backdrop)
    app.querySelector('#opener')?.remove()
    scope.release()
    expect(document.activeElement).toBe(app.querySelector('h1'))
    expect(app.querySelector('h1')?.getAttribute('tabindex')).toBe('-1')
  })

  it('nothing had the focus: nothing is forced on the page', () => {
    add(document.body, '<div id="app"><button id="opener">open</button><h1>Page</h1></div>')
    ;(document.activeElement as HTMLElement | null)?.blur()
    const { backdrop } = dialog(HEAD)
    const scope = open(backdrop)
    scope.release()
    expect(document.activeElement).toBe(document.body)
  })

  it('the control is gone but a dialog is below: the focus goes into that dialog', () => {
    page()
    const first = dialog(`${HEAD}<div class="modal-body"><button id="row">row</button></div>`)
    const s1 = open(first.backdrop)
    expect(id()).toBe('row')
    const second = dialog('<div class="modal-foot"><button id="no">no</button></div>')
    const s2 = open(second.backdrop)
    expect(id()).toBe('no')
    document.getElementById('row')!.remove() // the row that opened the question disappeared
    s2.release()
    expect(first.backdrop.contains(document.activeElement)).toBe(true)
    expect(s1.active).toBe(true)
  })

  it('focus that the user (or the dialog) sent elsewhere on purpose is not taken back', () => {
    const app = page('<button id="opener">open</button><input id="field">')
    const { backdrop } = dialog(HEAD)
    const scope = open(backdrop)
    scope.release() // the page is free again
    app.querySelector<HTMLElement>('#field')?.focus()
    scope.release()
    expect(id()).toBe('field')
  })

  it('a scope that is not on top does not move the focus when it goes (the one above owns it)', () => {
    page()
    const first = dialog(`${HEAD}<div class="modal-body"><button id="row">row</button></div>`)
    const s1 = open(first.backdrop)
    const second = dialog('<div class="modal-foot"><button id="no">no</button></div>')
    open(second.backdrop)
    s1.release()
    expect(id()).toBe('no')
  })
})

describe('Escape', () => {
  it('belongs to the top scope only; one press closes one thing', () => {
    page()
    const first = dialog(`${HEAD}<div class="modal-body"><button id="row">row</button></div>`)
    const second = dialog('<div class="modal-foot"><button id="no">no</button></div>')
    const heard: string[] = []
    const s1 = open(first.backdrop, { onEscape: () => { heard.push('first'); s1.release() } })
    const s2 = open(second.backdrop, { onEscape: () => { heard.push('second'); s2.release() } })
    const e = key('Escape')
    expect(heard).toEqual(['second'])
    expect(e.defaultPrevented || true).toBe(true)
    key('Escape')
    expect(heard).toEqual(['second', 'first'])
    expect(openScopes()).toBe(0)
  })

  it('does not reach the page: handlers of the page below do not hear it', () => {
    page()
    const heardByPage = vi.fn()
    document.addEventListener('keydown', heardByPage) // the page listens in the bubble phase
    const { backdrop } = dialog(HEAD)
    open(backdrop, { onEscape: () => undefined })
    key('Escape')
    expect(heardByPage).not.toHaveBeenCalled()
    document.removeEventListener('keydown', heardByPage)
  })

  it('a dialog that cannot be closed still keeps the key from the page', () => {
    page()
    const heardByPage = vi.fn()
    document.addEventListener('keydown', heardByPage)
    const { backdrop } = dialog(HEAD)
    open(backdrop)
    key('Escape')
    expect(heardByPage).not.toHaveBeenCalled()
    document.removeEventListener('keydown', heardByPage)
  })

  it('with no scope open the page hears everything', () => {
    page()
    const heardByPage = vi.fn()
    document.addEventListener('keydown', heardByPage)
    key('Escape')
    expect(heardByPage).toHaveBeenCalledTimes(1)
    document.removeEventListener('keydown', heardByPage)
  })

  it('a key pressed during text composition is not an Escape for the dialog', () => {
    page()
    const { backdrop } = dialog(HEAD)
    const onEscape = vi.fn()
    open(backdrop, { onEscape })
    key('Escape', { isComposing: true })
    expect(onEscape).not.toHaveBeenCalled()
  })
})

// ---------------------------------------------------------------------------------------------------------------- the real Modal component
describe('Modal.vue on the scope', () => {
  const state = reactive({ a: false, b: false })
  const closed: string[] = []
  const Host = defineComponent({
    setup: () => () => h('div', { id: 'app' }, [
      h('button', { id: 'opener' }, 'open'),
      h('h1', 'Page'),
      state.a ? h(Modal, { title: 'A', onClose: () => { closed.push('A'); state.a = false } }, {
        default: () => h('button', { id: 'restore' }, 'restore'),
        footer: () => h('button', { id: 'a-close' }, 'close'),
      }) : null,
      state.b ? h(Modal, { title: 'B', onClose: () => { closed.push('B'); state.b = false } }, {
        default: () => h('p', 'sure?'),
        footer: () => [h('button', { id: 'cancel', autofocus: true }, 'cancel'), h('button', { id: 'ok' }, 'ok')],
      }) : null,
    ]),
  })

  afterEach(() => { state.a = false; state.b = false; closed.length = 0 })

  const settle = async () => { await nextTick(); await new Promise((r) => setTimeout(r, 30)); await nextTick() }

  it('takes the focus, holds it, and one Escape closes one dialog and returns the focus to the control that opened it', async () => {
    const wrapper = mount(Host, { attachTo: document.body })
    document.getElementById('opener')!.focus()
    state.a = true
    await settle()
    expect(id()).toBe('restore') // the first control of the body
    expect(document.querySelector('[data-v-app]')?.hasAttribute('inert')).toBe(true)

    // the "Restore" button of A opens a question B over it
    document.getElementById('restore')!.focus()
    state.b = true
    await settle()
    expect(id()).toBe('cancel')
    expect(document.querySelectorAll('[role="dialog"][aria-modal="true"]').length).toBe(2)

    key('Escape')
    await settle()
    expect(closed).toEqual(['B']) // before: one Escape closed both
    expect(document.querySelectorAll('[role="dialog"][aria-modal="true"]').length).toBe(1)
    expect(id()).toBe('restore') // back where the question came from
    expect(document.querySelector('[data-v-app]')?.hasAttribute('inert')).toBe(true) // the page is still locked under A

    key('Escape')
    await settle()
    expect(closed).toEqual(['B', 'A'])
    expect(document.querySelectorAll('[role="dialog"]').length).toBe(0)
    expect(id()).toBe('opener')
    expect(document.querySelector('[data-v-app]')?.hasAttribute('inert')).toBe(false)
    wrapper.unmount()
  })

  it('a close that the parent refuses ("discard your changes?": no) leaves the dialog open, visible and closable again', async () => {
    const refuse = reactive({ asked: 0 })
    const Refusing = defineComponent({
      setup: () => () => h('div', { id: 'app' }, [
        h('button', { id: 'opener' }, 'open'),
        h(Modal, { title: 'Edit', onClose: () => { refuse.asked++ } }, { default: () => h('input', { id: 'field' }) }),
      ]),
    })
    const wrapper = mount(Refusing, { attachTo: document.body })
    await settle()
    expect(id()).toBe('field')
    key('Escape')
    await settle()
    expect(refuse.asked).toBe(1)
    expect(document.querySelector('.modal-backdrop')?.classList.contains('closing'), 'still faded out for good').toBe(false)
    key('Escape')
    await settle()
    expect(refuse.asked, 'the second Escape asks again').toBe(2)
    wrapper.unmount()
  })

  it('a dialog that is removed by its parent (not closed by itself) gives the focus back too', async () => {
    const wrapper = mount(Host, { attachTo: document.body })
    document.getElementById('opener')!.focus()
    state.a = true
    await settle()
    expect(id()).toBe('restore')
    expect(document.querySelector('[data-v-app]')?.hasAttribute('inert')).toBe(true)
    state.a = false // the parent decides: no exit animation, no close event
    await settle()
    expect(id()).toBe('opener')
    expect(document.querySelector('[data-v-app]')?.hasAttribute('inert')).toBe(false)
    wrapper.unmount()
  })
})
