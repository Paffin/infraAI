<script setup lang="ts">
import { computed } from 'vue'
import { X } from 'lucide-vue-next'
import { t } from '@/i18n'
import { eventText, scenarioLabel } from '@/lib/sim'
import { useEditor } from '@/stores/editor'

const editor = useEditor()
const sim = computed(() => editor.sim)
const res = computed(() => sim.value?.result)
const title = computed(() => (sim.value ? scenarioLabel(sim.value.scenario, editor.arch, editor.report?.units) : ''))
</script>

<template>
  <div v-if="sim && res" class="simcard card" role="status">
    <header>
      <span class="dot" /><b class="grow truncate">{{ t('sim.banner', { failed: res.failed.length ? res.failed.join(', ') : title }) }}</b>
      <button class="btn btn-ghost btn-icon" :aria-label="t('sim.exit')" :title="t('sim.exit')" @click="editor.exitSim()"><X :size="16" /></button>
    </header>
    <p class="state" :class="res.state">{{ t('sim.result.' + res.state) }}<span v-if="res.downUnits.length"> · {{ res.downUnits.join(', ') }}</span></p>
    <h5>{{ t('sim.details') }}</h5>
    <ul>
      <li v-for="(e, i) in res.events" :key="i">{{ eventText(e) }}</li>
      <li v-if="!res.events.length" class="muted">{{ t('sim.noevents') }}</li>
    </ul>
    <p v-if="res.rpoAssumed" class="muted small">{{ t('sim.assumed') }}</p>
  </div>
</template>

<style scoped>
.simcard { position: absolute; right: 18px; top: 62px; width: 330px; max-height: calc(100% - 270px); overflow: auto; z-index: 55; padding: 12px 16px 14px; box-shadow: var(--shadow-2); border-color: var(--crit-line); }
header { display: flex; align-items: center; gap: 8px; margin: -4px -8px 2px 0; font-size: 14px; }
.dot { width: 9px; height: 9px; border-radius: 50%; background: var(--crit); flex: none; }
.state { margin: 0 0 8px; font-weight: 600; font-size: 13px; }
.state.up { color: var(--ok); } .state.degraded { color: var(--risk); } .state.down { color: var(--crit); }
h5 { margin: 6px 0; font-size: 10.5px; letter-spacing: .09em; text-transform: uppercase; color: #6e7fa8; font-weight: 600; }
ul { margin: 0; padding-left: 16px; display: flex; flex-direction: column; gap: 6px; font-size: 12.5px; line-height: 1.45; color: var(--text-2); }
.small { font-size: 12px; margin: 8px 0 0; }
@container (max-width: 1260px) { .simcard { top: 104px; max-height: calc(100% - 280px); } }
</style>
