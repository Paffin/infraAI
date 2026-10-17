import { mount } from '@vue/test-utils'
import { afterEach, describe, expect, it } from 'vitest'
import { computed, defineComponent, h, nextTick, ref } from 'vue'
import { useTabs } from '../src/lib/useTabs'

// Real DOM (jsdom) and the real composable. jsdom has no layout, so what is judged is the contract of the pattern: who has tabindex 0, what the arrows and Home / End do
// to the focus and to the selection, how a tab and its panel name each other. That a browser really walks it is proven in e2e/tabs.spec.ts on the four lists.

const mounted: ReturnType<typeof mount>[] = []
afterEach(() => { for (const m of mounted.splice(0)) m.unmount(); document.body.innerHTML = '' })

/** A list of tabs and the panel of the selected one, the way the panels of the editor use it. `ids` can change under it (the failure tab of the right panel). */
function setup(initial: string[] = ['checks', 'requirements', 'analysis'], selected = 'checks') {
  const ids = ref(initial)
  const sel = ref(selected)
  const Comp = defineComponent({
    setup() {
      const { tablist, tab, panel } = useTabs({ tabs: ids, selected: sel, select: (id) => { sel.value = id }, label: computed(() => 'Sections') })
      return () => h('div', [
        h('div', { class: 'list', ...tablist.value }, ids.value.map((id) => h('button', { key: id, class: 'tab', ...tab(id) }, id))),
        h('div', { class: 'panel', ...panel.value }, 'panel of ' + sel.value),
      ])
    },
  })
  const wrapper = mount(Comp, { attachTo: document.body })
  mounted.push(wrapper)
  const tabs = () => Array.from(document.querySelectorAll<HTMLElement>('[role="tab"]'))
  const press = (key: string, target: Element | null = document.activeElement, init: KeyboardEventInit = {}) => {
    const e = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init })
    ;(target ?? document.body).dispatchEvent(e)
    return e
  }
  return { ids, sel, tabs, press, wrapper }
}

describe('the tab pattern', () => {
  it('has one stop of the Tab key, the selected tab, and names the list', () => {
    const { tabs } = setup()
    expect(tabs().map((t) => t.getAttribute('tabindex'))).toEqual(['0', '-1', '-1'])
    expect(tabs().map((t) => t.getAttribute('aria-selected'))).toEqual(['true', 'false', 'false'])
    const list = document.querySelector('[role="tablist"]')!
    expect(list.getAttribute('aria-label')).toBe('Sections')
    expect(new Set(tabs().map((t) => t.id)).size).toBe(3)
  })

  it('names the tab and the panel for each other, and promises a panel only for the selected tab', () => {
    const { tabs } = setup()
    const panel = document.querySelector('[role="tabpanel"]')!
    expect(panel.id).toBeTruthy()
    expect(panel.getAttribute('aria-labelledby')).toBe(tabs()[0].id)
    expect(tabs()[0].getAttribute('aria-controls')).toBe(panel.id)
    // the other panels are not in the page: a reference to them would be a reference to nothing
    expect(tabs().slice(1).map((t) => t.getAttribute('aria-controls'))).toEqual([null, null])
  })

  it('two lists on one page do not share an id', () => {
    const List = defineComponent({
      props: { names: { type: Array as () => string[], required: true } },
      setup(props) {
        const sel = ref(props.names[0])
        const { tablist, tab, panel } = useTabs({ tabs: computed(() => props.names), selected: sel, select: (id) => { sel.value = id } })
        return () => h('div', [h('div', tablist.value, props.names.map((n) => h('button', tab(n), n))), h('div', panel.value)])
      },
    })
    // one application, as the page is: the identifiers come from it
    const both = mount(defineComponent({ render: () => h('div', [h(List, { names: ['a', 'b', 'c'] }), h(List, { names: ['x', 'y'] })]) }), { attachTo: document.body })
    mounted.push(both)
    const ids = Array.from(document.querySelectorAll('[role="tab"], [role="tabpanel"]')).map((e) => e.id)
    expect(ids).toHaveLength(7)
    expect(new Set(ids).size).toBe(7)
  })

  it('moves the focus and the selection together with the arrows, and wraps', async () => {
    const { tabs, sel, press } = setup()
    tabs()[0].focus()
    press('ArrowRight')
    await nextTick()
    expect(document.activeElement).toBe(tabs()[1])
    expect(sel.value).toBe('requirements')
    expect(tabs().map((t) => t.getAttribute('tabindex'))).toEqual(['-1', '0', '-1'])
    press('ArrowRight')
    press('ArrowRight') // past the last: the first
    await nextTick()
    expect(document.activeElement).toBe(tabs()[0])
    expect(sel.value).toBe('checks')
    press('ArrowLeft') // before the first: the last
    await nextTick()
    expect(document.activeElement).toBe(tabs()[2])
    expect(sel.value).toBe('analysis')
  })

  it('goes to the first and the last with Home and End', async () => {
    const { tabs, sel, press } = setup(['a', 'b', 'c', 'd'], 'b')
    tabs()[1].focus()
    press('End')
    await nextTick()
    expect(sel.value).toBe('d')
    expect(document.activeElement).toBe(tabs()[3])
    press('Home')
    await nextTick()
    expect(sel.value).toBe('a')
    expect(document.activeElement).toBe(tabs()[0])
  })

  it('keeps the arrows to itself, and leaves the keys it does not own', () => {
    const { tabs, press } = setup()
    tabs()[0].focus()
    const arrow = press('ArrowRight')
    expect(arrow.defaultPrevented).toBe(true)
    // a key with a modifier is the browser's (Alt+Left is "back"), and other keys are nobody's here
    expect(press('ArrowRight', tabs()[1], { altKey: true }).defaultPrevented).toBe(false)
    expect(press('ArrowRight', tabs()[1], { shiftKey: true }).defaultPrevented).toBe(false)
    expect(press('a', tabs()[1]).defaultPrevented).toBe(false)
  })

  it('walks the tabs that are drawn, in the order of the page', async () => {
    const { tabs, sel, press } = setup(['a', 'b', 'c'], 'a')
    tabs()[1].style.display = 'none' // folded into a menu on a narrow window
    tabs()[0].focus()
    press('ArrowRight')
    await nextTick()
    expect(sel.value).toBe('c')
    press('ArrowLeft')
    await nextTick()
    expect(sel.value).toBe('a')
  })

  it('a click selects, as a key does', async () => {
    const { tabs, sel } = setup()
    tabs()[2].click()
    await nextTick()
    expect(sel.value).toBe('analysis')
    expect(tabs().map((t) => t.getAttribute('tabindex'))).toEqual(['-1', '-1', '0'])
  })

  it('a tab that goes while it has the focus hands the focus to the tab that is shown, not to the page', async () => {
    const { ids, sel, tabs } = setup(['sim', 'checks', 'analysis'], 'sim')
    tabs()[0].focus()
    expect(document.activeElement).toBe(tabs()[0])
    // the failure is closed: its tab goes, the panel falls back to another one
    ids.value = ['checks', 'analysis']
    sel.value = 'checks'
    await nextTick()
    await nextTick()
    expect(document.activeElement).toBe(tabs()[0])
    expect(tabs()[0].dataset.tab).toBe('checks')
    // a tab that goes while the focus is elsewhere does not take the focus
    const outside = document.createElement('button')
    document.body.appendChild(outside)
    outside.focus()
    ids.value = ['checks']
    await nextTick()
    await nextTick()
    expect(document.activeElement).toBe(outside)
  })
})
