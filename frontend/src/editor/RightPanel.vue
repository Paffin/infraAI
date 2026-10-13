<script setup lang="ts">
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
</script>

<template>
  <aside class="right">
    <div class="tabs" role="tablist">
      <button v-for="x in tabs" :key="x" class="tab" :class="{ active: ui.tab === x }" role="tab" :aria-selected="ui.tab === x" @click="ui.tab = x">
        {{ t('tab.' + x) }}<span v-if="x === 'chat' && ai.busy" class="spinner pip" />
      </button>
    </div>
    <ChecksPanel v-if="ui.tab === 'checks'" />
    <RequirementsPanel v-else-if="ui.tab === 'requirements'" />
    <CapacityPanel v-else-if="ui.tab === 'capacity'" />
    <CostPanel v-else-if="ui.tab === 'cost'" />
    <ChatPanel v-else />
  </aside>
</template>

<style scoped>
.right { width: var(--rightpanel-w); flex: none; background: var(--surface); border-left: 1px solid var(--line); display: flex; flex-direction: column; min-height: 0; }
.tabs { padding: 0 8px 0 12px; gap: 0; overflow-x: auto; scrollbar-width: none; flex: none; }
.tabs::-webkit-scrollbar { display: none; }
.tab { white-space: nowrap; padding: 0 7px; font-size: 13px; }
.pip { margin-left: 6px; width: 11px; height: 11px; }
</style>
