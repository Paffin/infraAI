<script setup lang="ts">
import { computed } from 'vue'
import Modal from '@/components/Modal.vue'
import { t } from '@/i18n'
import { fmtDuration, fmtPct } from '@/lib/format'
import { resultChips, scenarioLabel } from '@/lib/sim'
import { useEditor } from '@/stores/editor'
import { useUi } from '@/stores/ui'

const editor = useEditor()
const ui = useUi()
const rows = computed(() => (editor.report?.scenarios ?? []).map((s) => ({
  s,
  label: s.failed.length ? s.failed.join(' + ') : scenarioLabel(s.scenario, editor.arch, editor.report?.units),
  tone: s.state === 'up' ? 'tag-ok' : s.state === 'degraded' ? 'tag-risk' : 'tag-crit',
  flags: resultChips({ state: s.state, recovery: { kind: s.recoveryKind, seconds: s.recoverySeconds }, rpoSeconds: s.rpoSeconds, rpoAssumed: s.rpoAssumed, capacityRatio: s.capacityRatio, splitBrain: s.splitBrain, dataLost: s.dataLost }).filter((c) => c.tone === 'crit'),
})))

function replay(i: number) {
  ui.modal = null
  void editor.runScenario(rows.value[i].s.scenario)
}
const recovery = (k: string, sec: number | null) => (k === 'auto' ? t('sim.result.switch', { s: fmtDuration(sec) }) : k === 'manual' ? t('sim.result.manual', { s: fmtDuration(sec) }) : k === 'never' ? t('sim.result.never') : '—')
</script>

<template>
  <Modal :title="t('sim.matrix.title')" size="lg" @close="ui.modal = null">
    <p v-if="!rows.length" class="muted">{{ t('sim.matrix.empty') }}</p>
    <table v-else class="table">
      <thead><tr><th>{{ t('sim.col.scenario') }}</th><th>{{ t('sim.col.state') }}</th><th>{{ t('sim.col.recovery') }}</th><th>{{ t('sim.col.rpo') }}</th><th>{{ t('sim.col.capacity') }}</th><th /></tr></thead>
      <tbody>
        <tr v-for="(r, i) in rows" :key="i" class="clickable" @click="replay(i)">
          <td><b>{{ r.label }}</b></td>
          <td><span class="tag" :class="r.tone">{{ t('sim.result.' + r.s.state) }}</span></td>
          <td>{{ recovery(r.s.recoveryKind, r.s.recoverySeconds) }}</td>
          <td class="mono">{{ r.s.rpoSeconds === null ? '?' : r.s.rpoSeconds === 0 ? '0' : fmtDuration(r.s.rpoSeconds) }}<span v-if="r.s.rpoAssumed && r.s.rpoSeconds" :title="t('sim.assumed')"> ~</span></td>
          <td class="mono">{{ fmtPct(r.s.capacityRatio) }}</td>
          <td><span v-for="f in r.flags" :key="f.text" class="tag tag-crit" style="margin-right: 4px">{{ f.text }}</span></td>
        </tr>
      </tbody>
    </table>
  </Modal>
</template>

<style scoped>
.clickable { cursor: pointer; }
.tag { white-space: nowrap; }
</style>
