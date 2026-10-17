<script setup lang="ts">
import { computed, type Component } from 'vue'
import { t } from '@/i18n'
import { useTabs } from '@/lib/useTabs'
import { ANALYSIS_VIEWS, useUi, type AnalysisView } from '@/stores/ui'
import AvailabilityPanel from './AvailabilityPanel.vue'
import CapacityPanel from './CapacityPanel.vue'
import CostPanel from './CostPanel.vue'
import SitesPanel from './SitesPanel.vue'

const ui = useUi()
const panels: Record<AnalysisView, Component> = { capacity: CapacityPanel, availability: AvailabilityPanel, cost: CostPanel, sites: SitesPanel }
// the tab pattern (lib/useTabs.ts); the panel under the tabs gets the attributes of the tab panel, its root is the element they land on
const { tablist, tab: tabAttrs, panel: panelAttrs } = useTabs({ tabs: computed(() => [...ANALYSIS_VIEWS]), selected: computed(() => ui.analysis), select: (v) => { ui.analysis = v as AnalysisView }, label: computed(() => t('tab.analysis')) })
</script>

<template>
  <div class="analysis">
    <div class="seg" v-bind="tablist">
      <button v-for="v in ANALYSIS_VIEWS" :key="v" class="segbtn" :class="{ on: ui.analysis === v }" v-bind="tabAttrs(v)" :data-view="v">
        {{ t('tab.' + v) }}
      </button>
    </div>
    <Transition name="swap" mode="out-in">
      <component :is="panels[ui.analysis]" :key="ui.analysis" v-bind="panelAttrs" />
    </Transition>
  </div>
</template>

<style scoped>
.analysis { display: flex; flex-direction: column; min-height: 0; flex: 1; }
.seg { display: flex; gap: 2px; margin: 12px 20px 0; padding: 3px; background: var(--surface-sunken); border-radius: var(--radius); flex: none; }
.segbtn { flex: 1; display: inline-flex; align-items: center; justify-content: center; gap: 6px; height: 30px; padding: 0 6px; border: 0; border-radius: 6px; background: transparent; color: var(--text-2); font-size: 13px; white-space: nowrap; transition: background-color var(--dur-fast), color var(--dur-fast), box-shadow var(--dur-fast); }
.segbtn:hover:not(.on) { color: var(--text); background: rgba(255, 255, 255, .5); }
.segbtn.on { background: var(--surface); color: var(--accent); font-weight: 600; box-shadow: var(--shadow-1); }
</style>
