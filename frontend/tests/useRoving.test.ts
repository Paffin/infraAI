import { mount } from '@vue/test-utils'
import { afterEach, describe, expect, it } from 'vitest'
import { defineComponent, h, nextTick, ref } from 'vue'
import { useRoving } from '../src/lib/useRoving'

// Real DOM (jsdom) and the real composable: which button has tabindex 0, what the arrows do, what is left after the list changes under it. That a browser's Tab key
// really leaves the list after one stop is proven in e2e/keyboard.spec.ts on the real library.

const mounted: ReturnType<typeof mount>[] = []
afterEach(() => { for (const m of mounted.splice(0)) m.unmount(); document.body.innerHTML = '' })

/** A list shaped like the library: headers that fold a group (`aria-expanded`), and the buttons of a group while it is open. */
function setup(open = true) {
  const groupOpen = ref(open)
  const query = ref('')
  const Comp = defineComponent({
    setup() {
      const scroll = ref<HTMLElement | null>(null)
      useRoving(scroll, 'button')
      return () => h('div', [
        h('input', { class: 'search' }),
        h('div', { ref: scroll, class: 'scroll' }, [
          h('button', { class: 'ch', 'aria-expanded': String(groupOpen.value), onClick: () => { groupOpen.value = !groupOpen.value } }, 'Group'),
          ...(groupOpen.value ? ['one', 'two', 'three'].filter((x) => x.includes(query.value)).map((x) => h('button', { key: x, class: 'tile', id: x }, x)) : []),
          h('button', { class: 'last', disabled: false }, 'last'),
          h('button', { class: 'off', disabled: true }, 'disabled'),
        ]),
      ])
    },
  })
  const wrapper = mount(Comp, { attachTo: document.body })
  mounted.push(wrapper)
  const items = () => Array.from(document.querySelectorAll<HTMLElement>('.scroll button'))
  const stops = () => items().filter((b) => b.getAttribute('tabindex') === '0')
  const press = (key: string, init: KeyboardEventInit = {}) => {
    const e = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init })
    ;(document.activeElement ?? document.body).dispatchEvent(e)
    return e
  }
  return { groupOpen, query, items, stops, press }
}

describe('a long list as one stop of the Tab key', () => {
  it('gives the first button the stop and takes it from all the others', () => {
    const { items, stops } = setup()
    expect(stops()).toHaveLength(1)
    expect(stops()[0]).toBe(items()[0])
    expect(items().filter((b) => b.getAttribute('tabindex') === '-1')).toHaveLength(items().length - 1 - 1) // the disabled one is nobody's
  })

  it('moves the stop with the focus', () => {
    const { items, stops } = setup()
    items()[2].focus()
    expect(stops()).toEqual([items()[2]])
  })

  it('walks the buttons in the order of the page with the arrows, Home and End, and stops at the ends', () => {
    const { items, stops, press } = setup()
    items()[0].focus()
    expect(press('ArrowDown').defaultPrevented).toBe(true)
    expect(document.activeElement).toBe(items()[1])
    press('ArrowDown')
    press('ArrowUp')
    expect(document.activeElement).toBe(items()[1])
    press('End')
    expect(document.activeElement).toBe(items()[4]) // "last"; the disabled button is not a stop
    press('ArrowDown')
    expect(document.activeElement).toBe(items()[4])
    press('Home')
    expect(document.activeElement).toBe(items()[0])
    press('ArrowUp')
    expect(document.activeElement).toBe(items()[0])
    expect(stops()).toEqual([items()[0]])
  })

  it('opens and closes a header with Right and Left, and does not do it twice', async () => {
    const { groupOpen, items, press } = setup(true)
    items()[0].focus()
    press('ArrowRight') // already open: nothing
    await nextTick()
    expect(groupOpen.value).toBe(true)
    press('ArrowLeft')
    await nextTick()
    expect(groupOpen.value).toBe(false)
    press('ArrowLeft')
    await nextTick()
    expect(groupOpen.value).toBe(false)
    press('ArrowRight')
    await nextTick()
    expect(groupOpen.value).toBe(true)
  })

  it('never leaves the list without its one stop: the group folds, a search narrows the list, the stop itself goes', async () => {
    const { groupOpen, query, items, stops } = setup()
    items()[3].focus() // "three"
    expect(stops()[0].id).toBe('three')
    // the group folds: the stop is gone with its button
    groupOpen.value = false
    await nextTick()
    await Promise.resolve() // the observer
    expect(stops()).toHaveLength(1)
    groupOpen.value = true
    query.value = 'o' // "one" and "two"
    await nextTick()
    await Promise.resolve()
    expect(stops()).toHaveLength(1)
    query.value = 'zzz' // nothing left but the header and the last button
    await nextTick()
    await Promise.resolve()
    expect(stops()).toHaveLength(1)
    expect(items().filter((b) => !b.hasAttribute('disabled'))).toContain(stops()[0])
  })

  it('leaves the keys with a modifier to the browser', () => {
    const { items, press } = setup()
    items()[0].focus()
    expect(press('ArrowDown', { ctrlKey: true }).defaultPrevented).toBe(false)
    expect(press('ArrowDown', { shiftKey: true }).defaultPrevented).toBe(false)
    expect(document.activeElement).toBe(items()[0])
  })
})
