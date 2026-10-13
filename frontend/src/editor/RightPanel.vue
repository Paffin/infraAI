<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { t } from '@/i18n'
import { useAi } from '@/stores/ai'
import { useUi, type RightTab } from '@/stores/ui'
import CapacityPanel from './CapacityPanel.vue'
import ChatPanel from './ChatPanel.vue'
import ChecksPanel from './ChecksPanel.vue'
import CostPanel from './CostPanel.vue'
import RequirementsPanel from './RequirementsPanel.vue'

const ui = useUi()
const ai = useAi()
const tabs: RightTab[] = ['checks', 'requirements', 'capacity', 'cost', 'chat']
const panels = { checks: ChecksPanel, requirements: RequirementsPanel, capacity: CapacityPanel, cost: CostPanel, chat: ChatPanel }
const panel = computed(() => panels[ui.tab] ?? ChatPanel)

// The underline slides to the active tab; where it is comes from the tab's own box, so any font or language fits.
const bar = ref<HTMLElement | null>(null)
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
  <aside class="right">
    <div ref="bar" class="tabs" role="tablist">
      <button v-for="x in tabs" :key="x" class="tab" :class="{ active: ui.tab === x }" role="tab" :aria-selected="ui.tab === x" @click="ui.tab = x">
        {{ t('tab.' + x) }}<span v-if="x === 'chat' && ai.busy" class="spinner pip" />
      </button>
      <span class="ink" :class="{ ready: ink.ready }" :style="{ width: ink.w + 'px', transform: `translateX(${ink.x}px)` }" aria-hidden="true" />
    </div>
    <Transition name="swap" mode="out-in">
      <component :is="panel" :key="ui.tab" />
    </Transition>
  </aside>
</template>

<style scoped>
.right { width: var(--rightpanel-w); flex: none; background: var(--surface); border-left: 1px solid var(--line); display: flex; flex-direction: column; min-height: 0; }
.tabs { padding: 0 8px 0 12px; gap: 0; overflow-x: auto; scrollbar-width: none; flex: none; position: relative; }
.tabs::-webkit-scrollbar { display: none; }
.tab { white-space: nowrap; padding: 0 7px; font-size: 13px; }
.tab.active { border-bottom-color: transparent; }
.ink { position: absolute; left: 0; bottom: -1px; height: 2px; background: var(--accent); border-radius: 2px 2px 0 0; pointer-events: none; }
.ink.ready { transition: transform var(--dur-base) var(--ease-out), width var(--dur-base) var(--ease-out); }
.pip { margin-left: 6px; width: 11px; height: 11px; }
</style>
