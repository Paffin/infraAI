<script setup lang="ts">
// The failure panel as a window over the diagram: the same card as the "Отказ" tab of the right panel, which the user can drag anywhere,
// fold to its header (that still says what is happening: the state and the share of the traffic served) and put back into the right panel.
import { computed } from 'vue'
import FloatPanel from '@/components/FloatPanel.vue'
import { t } from '@/i18n'
import { scenarioLabel } from '@/lib/sim'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'
import { usePlayback } from '@/stores/playback'
import { useUi } from '@/stores/ui'
import SimDetails from './SimDetails.vue'

const app = useApp()
const editor = useEditor()
const playback = usePlayback()
const ui = useUi()

const res = computed(() => editor.sim?.result)
const title = computed(() => {
  const sim = editor.sim
  if (!sim || !res.value) return t('tab.sim')
  const label = scenarioLabel(sim.scenario, editor.arch, editor.report?.units)
  return t('sim.banner', { failed: res.value.failed.length ? res.value.failed.join(', ') : label })
})
/** The state of the picture on screen: the dot before the title and the pill after it follow the story. */
const state = computed(() => playback.current?.state ?? res.value?.state ?? null)
const tone = computed(() => (state.value === 'up' ? 'ok' : state.value === 'degraded' ? 'risk' : state.value === 'down' ? 'crit' : ''))
const servedPct = computed(() => (playback.served === null ? null : Math.round(playback.served * 100)))

function dock() {
  ui.simMode = 'dock'
  ui.rightOpen = true
}
</script>

<template>
  <FloatPanel v-model:folded="ui.simFolded" storage-key="ia_sim_pos" :width="app.editorCfg.floatPanelWidth" :title="title" :tone="tone" :dock-label="t('sim.dock')" @dock="dock" @close="editor.exitSim()">
    <template v-if="servedPct !== null" #actions>
      <span class="pill" :class="tone" :title="t('traffic.served')" data-testid="float-served">{{ servedPct }}%</span>
    </template>
    <SimDetails bare />
  </FloatPanel>
</template>

<style scoped>
.pill { flex: none; font-size: 11.5px; font-weight: 700; font-variant-numeric: tabular-nums; padding: 1px 8px; border-radius: 10px; background: var(--surface-sunken); color: var(--text-2); transition: background-color var(--dur-base), color var(--dur-base); }
.pill.ok { background: var(--ok-bg); color: var(--ok); }
.pill.risk { background: var(--risk-bg); color: var(--risk); }
.pill.crit { background: var(--crit-bg); color: var(--crit); }
</style>
