<script setup lang="ts">
import type { Component } from 'vue'
import { Activity, Coins, Gauge } from 'lucide-vue-next'
import { t } from '@/i18n'
import { ANALYSIS_VIEWS, useUi, type AnalysisView } from '@/stores/ui'
import AvailabilityPanel from './AvailabilityPanel.vue'
import CapacityPanel from './CapacityPanel.vue'
import CostPanel from './CostPanel.vue'

const ui = useUi()
const icons: Record<AnalysisView, Component> = { capacity: Gauge, availability: Activity, cost: Coins }
const panels: Record<AnalysisView, Component> = { capacity: CapacityPanel, availability: AvailabilityPanel, cost: CostPanel }
</script>

<template>
  <div class="analysis">
    <div class="seg" role="tablist" :aria-label="t('tab.analysis')">
      <button v-for="v in ANALYSIS_VIEWS" :key="v" class="segbtn" :class="{ on: ui.analysis === v }" role="tab" :aria-selected="ui.analysis === v" :data-view="v" @click="ui.analysis = v">
        <component :is="icons[v]" :size="14" aria-hidden="true" />{{ t('tab.' + v) }}
      </button>
    </div>
    <Transition name="swap" mode="out-in">
      <component :is="panels[ui.analysis]" :key="ui.analysis" />
    </Transition>
  </div>
</template>

<style scoped>
.analysis { display: flex; flex-direction: column; min-height: 0; flex: 1; }
.seg { display: flex; gap: 2px; margin: 12px 20px 0; padding: 3px; background: var(--surface-sunken); border-radius: var(--radius); flex: none; }
.segbtn { flex: 1; display: inline-flex; align-items: center; justify-content: center; gap: 6px; height: 30px; padding: 0 8px; border: 0; border-radius: 6px; background: transparent; color: var(--text-2); font-size: 13px; white-space: nowrap; transition: background-color var(--dur-fast), color var(--dur-fast), box-shadow var(--dur-fast); }
.segbtn:hover:not(.on) { color: var(--text); background: rgba(255, 255, 255, .5); }
.segbtn.on { background: var(--surface); color: var(--accent); font-weight: 600; box-shadow: var(--shadow-1); }
.segbtn svg { flex: none; }
</style>
