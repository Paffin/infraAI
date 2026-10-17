import { computed, nextTick, unref, useId, watch, type MaybeRef, type Ref } from 'vue'

/**
 * The tab pattern (WAI-ARIA "Tabs", automatic activation) for a list of buttons and the one panel that shows what the selected one stands for.
 *
 *  - ONE stop of the Tab key: the selected tab has `tabindex="0"`, the others `-1`; ArrowLeft / ArrowRight move to the previous / next tab (and wrap), Home / End
 *    to the first / last, and the tab that gets the focus is selected.
 *  - The tab and the panel name each other: `aria-labelledby` of the panel is the selected tab, `aria-controls` of the SELECTED tab is the panel. The other tabs have
 *    no `aria-controls`: their panels are not in the page (a panel is drawn for the selected tab only), and a reference to nothing is a violation.
 *  - The keys read the tabs from the page, in the order they are drawn and without the ones that are not drawn (a tab folded into a menu on a narrow window), so a
 *    list that comes and goes (the failure tab of the right panel) needs nothing more.
 *  - When the tab that has the focus goes (the failure is closed), the focus goes to the selected one: it does not fall to the page.
 *
 * Use: `v-bind="tabs.tablist"` on the element with the tabs, `v-bind="tabs.tab(id)"` on each button, `v-bind="tabs.panel"` on the root of the panel.
 */
export interface TabsOptions {
  /** The ids of the tabs, in the order they are drawn. */
  tabs: Readonly<Ref<readonly string[]>>
  selected: Readonly<Ref<string>>
  select: (id: string) => void
  /** The name of the list of tabs. */
  label?: MaybeRef<string>
}

/** Is the tab drawn: not `hidden`, not `display: none` on it or above it up to the list, not `visibility: hidden`? (Read from the styles, not from the layout: it works without one.) */
function drawn(el: HTMLElement, list: HTMLElement): boolean {
  for (let n: HTMLElement | null = el; n; n = n === list ? null : n.parentElement) {
    if (n.hidden || getComputedStyle(n).display === 'none') return false
  }
  return getComputedStyle(el).visibility !== 'hidden'
}

export function useTabs(o: TabsOptions) {
  const uid = useId()
  const tabId = (id: string) => `${uid}-tab-${id}`
  const panelId = `${uid}-panel`

  function onKeydown(e: KeyboardEvent) {
    if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey || !(e.currentTarget instanceof HTMLElement)) return
    const from = e.target instanceof HTMLElement ? e.target.closest<HTMLElement>('[role="tab"]') : null
    if (!from) return
    const all = Array.from(e.currentTarget.querySelectorAll<HTMLElement>('[role="tab"]')).filter((t) => drawn(t, e.currentTarget as HTMLElement))
    const at = all.indexOf(from)
    if (at < 0) return
    const to = e.key === 'ArrowRight' ? (at + 1) % all.length : e.key === 'ArrowLeft' ? (at - 1 + all.length) % all.length : e.key === 'Home' ? 0 : e.key === 'End' ? all.length - 1 : -1
    if (to < 0) return
    // the arrows are the list's: not the playback's, not the canvas's
    e.preventDefault()
    e.stopPropagation()
    const next = all[to]
    next.focus()
    if (next.dataset.tab) o.select(next.dataset.tab)
  }

  /** The tab that has the focus is about to go: the focus goes to the selected one once the page has changed, instead of to the page. */
  watch(o.tabs, (now) => {
    const active = document.activeElement
    const mine = active instanceof HTMLElement ? active.closest<HTMLElement>('[role="tab"]') : null
    if (mine?.dataset.tab && mine.id.startsWith(`${uid}-tab-`) && !now.includes(mine.dataset.tab)) {
      void nextTick(() => document.getElementById(tabId(o.selected.value))?.focus())
    }
  }, { flush: 'pre' })

  const tablist = computed(() => ({ role: 'tablist', 'aria-orientation': 'horizontal' as const, 'aria-label': unref(o.label), onKeydown }))
  const tab = (id: string) => {
    const on = o.selected.value === id
    return {
      role: 'tab', id: tabId(id), 'data-tab': id, 'aria-selected': on, tabindex: on ? 0 : -1, 'aria-controls': on ? panelId : undefined,
      onClick: () => o.select(id),
    }
  }
  const panel = computed(() => ({ role: 'tabpanel', id: panelId, 'aria-labelledby': tabId(o.selected.value) }))

  return { tablist, tab, panel, tabId, panelId }
}
