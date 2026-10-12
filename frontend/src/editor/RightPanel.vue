<script setup lang="ts">
import { t } from '@/i18n'
import { useAi } from '@/stores/ai'
import { useUi, type RightTab } from '@/stores/ui'
import ChatPanel from './ChatPanel.vue'
import ChecksPanel from './ChecksPanel.vue'
import CostPanel from './CostPanel.vue'
import RequirementsPanel from './RequirementsPanel.vue'

const ui = useUi()
const ai = useAi()
const tabs: RightTab[] = ['checks', 'requirements', 'cost', 'chat']
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
    <CostPanel v-else-if="ui.tab === 'cost'" />
    <ChatPanel v-else />
  </aside>
</template>

<style scoped>
.right { width: var(--rightpanel-w); flex: none; background: var(--surface); border-left: 1px solid var(--line); display: flex; flex-direction: column; min-height: 0; }
.tabs { padding: 0 12px 0 16px; gap: 2px; }
.tab { white-space: nowrap; padding: 0 9px; }
.pip { margin-left: 6px; width: 11px; height: 11px; }
</style>
