<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { Activity, ChartBar, ListChecks, MessageSquare, PanelRightClose, PanelRightOpen, SlidersHorizontal } from 'lucide-vue-next'
import { t } from '@/i18n'
import { clampWidth } from '@/lib/floating'
import { useApp } from '@/stores/app'
import { useAi } from '@/stores/ai'
import { useEditor } from '@/stores/editor'
import { useUi, type RightTab } from '@/stores/ui'
import { useTabs } from '@/lib/useTabs'
import AnalysisPanel from './AnalysisPanel.vue'
import ChatPanel from './ChatPanel.vue'
import ChecksPanel from './ChecksPanel.vue'
import RequirementsPanel from './RequirementsPanel.vue'
import SimDetails from './SimDetails.vue'

const ui = useUi()
const ai = useAi()
const editor = useEditor()
const app = useApp()
/** The failure on the diagram has a tab of its own (its story and the traffic) while it is on screen and is not detached into a window of its own. */
const tabs = computed<RightTab[]>(() => {
  const own: RightTab[] = ['checks', 'requirements', 'analysis']
  // the chat is the tab of the AI: a server without it has nothing to say there
  if (ui.chrome.chatTab) own.push('chat')
  return editor.sim && ui.simMode === 'dock' ? ['sim', ...own] : own
})
// a tab that is gone (AI switched off while the chat was open) must not stay the one that is shown
watch(() => ui.chrome.chatTab, (has) => { if (!has && ui.tab === 'chat') ui.tab = 'checks' }, { immediate: true })
// the tab pattern (lib/useTabs.ts): one Tab stop, the arrows and Home / End move between the tabs, the tab and the panel name each other
const bar = ref<HTMLElement | null>(null)
const { tablist, tab: tabAttrs, panel: panelAttrs } = useTabs({ tabs, selected: computed(() => ui.tab), select: (x) => { ui.tab = x as RightTab }, label: computed(() => t('right.tabs')) })
const RAIL_ICONS = { sim: Activity, checks: ListChecks, requirements: SlidersHorizontal, analysis: ChartBar, chat: MessageSquare } as const
const panels = { checks: ChecksPanel, requirements: RequirementsPanel, analysis: AnalysisPanel, chat: ChatPanel, sim: SimDetails }
const panel = computed(() => panels[ui.tab] ?? ChatPanel)

// Running a failure opens its tab; closing it (or the project) goes back to the tab the user was on. A panel the user folded stays folded: its rail
// shows the failure (the dot on the tab), and the caption over the diagram tells the story.
let before: RightTab = 'checks'
watch(() => !!editor.sim && ui.simMode === 'dock', (on) => {
  if (on) {
    if (ui.tab !== 'sim') before = ui.tab
    ui.tab = 'sim'
  } else if (ui.tab === 'sim') {
    ui.tab = before
  }
})
/** The count that rides on the rail icon of a tab: open findings of the checks. */
const badge = (x: RightTab) => {
  if (x !== 'checks' || !editor.report) return null
  const c = editor.report.counts
  return c.critical ? { n: c.critical, tone: 'crit' } : c.risk ? { n: c.risk, tone: 'risk' } : null
}
/** A tab of the rail opens the panel on it. */
function openTab(x: RightTab) {
  ui.tab = x
  ui.rightOpen = true
}

// ---- the width: the edge of the panel is dragged (a double click brings the default back; the arrow keys step it)
const resizing = ref(false)
const KEY_STEP = 24
function setWidth(px: number) {
  const c = app.editorCfg
  // the canvas keeps a third of the window at least
  ui.setRightWidth(clampWidth(px, c.rightPanelMinWidth, c.rightPanelMaxWidth, window.innerWidth, Math.round(window.innerWidth / 3)))
}
function startResize(e: PointerEvent) {
  if (e.button !== 0) return
  e.preventDefault()
  const el = e.currentTarget as HTMLElement
  el.setPointerCapture?.(e.pointerId)
  resizing.value = true
  const startX = e.clientX
  const startW = ui.rightWidth
  const move = (ev: PointerEvent) => setWidth(startW + (startX - ev.clientX))
  const up = (ev: PointerEvent) => {
    resizing.value = false
    el.releasePointerCapture?.(ev.pointerId)
    el.removeEventListener('pointermove', move)
    el.removeEventListener('pointerup', up)
    el.removeEventListener('pointercancel', up)
  }
  el.addEventListener('pointermove', move)
  el.addEventListener('pointerup', up)
  el.addEventListener('pointercancel', up)
}
function resizeKey(e: KeyboardEvent) {
  if (e.key === 'ArrowLeft') { e.preventDefault(); setWidth(ui.rightWidth + KEY_STEP * (e.shiftKey ? 4 : 1)) }
  else if (e.key === 'ArrowRight') { e.preventDefault(); setWidth(ui.rightWidth - KEY_STEP * (e.shiftKey ? 4 : 1)) }
  else if (e.key === 'Home') { e.preventDefault(); ui.setRightWidth(null) }
}

// The underline slides to the active tab; where it is comes from the tab's own box, so any font or language fits.
const ink = ref({ x: 0, w: 0, ready: false })
function place() {
  const el = bar.value?.querySelector<HTMLElement>('.tab.active')
  if (el) ink.value = { x: el.offsetLeft, w: el.offsetWidth, ready: true }
}
let ro: ResizeObserver | undefined
onMounted(() => {
  place()
  if (bar.value && typeof ResizeObserver !== 'undefined') { ro = new ResizeObserver(place); ro.observe(bar.value) }
})
onBeforeUnmount(() => ro?.disconnect())
watch(() => ui.tab, () => nextTick(place))
</script>

<template>
  <aside id="editor-checks" class="right" :class="{ folded: !ui.rightOpen, resizing }" :style="{ '--right-w': ui.rightWidth + 'px' }" data-tour="checks" :aria-label="t('right.title')" data-testid="right-panel">
    <template v-if="ui.rightOpen">
      <div class="edge" role="separator" aria-orientation="vertical" tabindex="0" :aria-valuenow="ui.rightWidth" :aria-valuemin="app.editorCfg.rightPanelMinWidth" :aria-valuemax="app.editorCfg.rightPanelMaxWidth"
           :aria-label="t('right.resize')" :title="t('right.resize.hint')" data-testid="right-edge" @pointerdown="startResize" @dblclick="ui.setRightWidth(null)" @keydown="resizeKey" />
      <div class="head">
        <div ref="bar" class="tabs" v-bind="tablist">
          <button v-for="x in tabs" :key="x" class="tab" :class="{ active: ui.tab === x }" v-bind="tabAttrs(x)">
            {{ t('tab.' + x) }}<span v-if="x === 'chat' && ai.busy" class="spinner pip" /><i v-if="x === 'sim'" class="rec" aria-hidden="true" />
          </button>
          <span class="ink" :class="{ ready: ink.ready }" :style="{ width: ink.w + 'px', transform: `translateX(${ink.x}px)` }" aria-hidden="true" />
        </div>
        <button class="fold" :title="t('right.fold') + ' (])'" :aria-label="t('right.fold')" data-testid="right-fold" @click="ui.rightOpen = false"><PanelRightClose :size="16" /></button>
      </div>
      <Transition name="swap" mode="out-in">
        <component :is="panel" :key="ui.tab" v-bind="panelAttrs" />
      </Transition>
    </template>
    <nav v-else class="rail" :aria-label="t('right.title')">
      <button class="rbtn unfold" :title="t('right.unfold') + ' (])'" :aria-label="t('right.unfold')" data-testid="right-unfold" @click="ui.rightOpen = true"><PanelRightOpen :size="17" /></button>
      <button v-for="x in tabs" :key="x" class="rbtn" :class="{ active: ui.tab === x }" :title="t('tab.' + x)" :aria-label="t('tab.' + x) + (badge(x) ? ': ' + badge(x)?.n : '')" @click="openTab(x)">
        <component :is="RAIL_ICONS[x]" :size="17" />
        <span v-if="badge(x)" class="badge" :class="badge(x)?.tone">{{ badge(x)?.n }}</span>
        <i v-if="x === 'sim'" class="rec" aria-hidden="true" />
        <span v-if="x === 'chat' && ai.busy" class="spinner pip" />
      </button>
    </nav>
  </aside>
</template>

<style scoped>
.right { position: relative; width: var(--right-w, var(--rightpanel-w)); max-width: 66vw; flex: none; background: var(--surface); border-left: 1px solid var(--line); display: flex; flex-direction: column; min-height: 0; transition: width var(--dur-base) var(--ease-out); }
.right.resizing { transition: none; user-select: none; }
.right.folded { width: var(--rail-w, 52px); }
/* the edge you take hold of: wider than it looks, lit while it is hovered, dragged or focused */
.edge { position: absolute; left: -4px; top: 0; bottom: 0; width: 9px; z-index: 5; cursor: col-resize; touch-action: none; }
.edge::after { content: ''; position: absolute; left: 3px; top: 0; bottom: 0; width: 3px; border-radius: 2px; background: transparent; transition: background-color var(--dur-fast); }
.edge:hover::after, .edge:focus-visible::after, .resizing .edge::after { background: var(--accent); }
.edge:focus-visible { outline: none; }
.head { position: relative; flex: none; min-width: 0; }
.head .tabs { padding-right: 46px; }
.fold { position: absolute; right: 8px; top: 7px; width: 30px; height: 30px; border: 0; background: transparent; color: var(--muted); border-radius: var(--radius-sm); display: inline-flex; align-items: center; justify-content: center; transition: background-color var(--dur-fast), color var(--dur-fast); }
.fold:hover { background: var(--surface-sunken); color: var(--text); }
/* folded: a rail of the tabs, each opens the panel on it */
.rail { display: flex; flex-direction: column; align-items: center; gap: 4px; padding: 10px 0; }
.rbtn { position: relative; width: 36px; height: 36px; border: 0; border-radius: var(--radius); background: transparent; color: var(--muted); display: inline-flex; align-items: center; justify-content: center; transition: background-color var(--dur-fast), color var(--dur-fast); }
.rbtn:hover { background: var(--surface-sunken); color: var(--text); }
.rbtn.active { background: var(--accent-soft); color: var(--accent); }
.rbtn.unfold { margin-bottom: 6px; border: 1px solid var(--line); color: var(--text-2); }
.rbtn .badge { position: absolute; top: 1px; right: 0; min-width: 16px; height: 16px; padding: 0 4px; border-radius: 8px; font-size: 10px; font-weight: 700; line-height: 16px; text-align: center; color: #fff; font-variant-numeric: tabular-nums; }
.rbtn .badge.crit { background: var(--crit); }
.rbtn .badge.risk { background: var(--risk); }
.rbtn .rec { position: absolute; top: 4px; right: 4px; margin: 0; }
.rbtn .pip { position: absolute; top: 4px; right: 4px; margin: 0; width: 9px; height: 9px; }
.tabs { padding: 0 8px 0 12px; gap: 0; overflow-x: auto; scrollbar-width: none; flex: none; position: relative; }
.tabs::-webkit-scrollbar { display: none; }
.tab { white-space: nowrap; padding: 0 7px; font-size: 13px; }
.tab.active { border-bottom-color: transparent; }
.ink { position: absolute; left: 0; bottom: -1px; height: 2px; background: var(--accent); border-radius: 2px 2px 0 0; pointer-events: none; }
.ink.ready { transition: transform var(--dur-base) var(--ease-out), width var(--dur-base) var(--ease-out); }
.pip { margin-left: 6px; width: 11px; height: 11px; }
.rec { display: inline-block; width: 7px; height: 7px; margin-left: 6px; border-radius: 50%; background: var(--crit); box-shadow: 0 0 0 3px var(--crit-bg); vertical-align: 1px; }
</style>
