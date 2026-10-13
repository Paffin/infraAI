<script setup lang="ts">
import { computed } from 'vue'
import Modal from '@/components/Modal.vue'
import { lt, t } from '@/i18n'
import { statusLabel, statusTagClass } from '@/lib/findings'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'
import { useUi } from '@/stores/ui'

const app = useApp()
const editor = useEditor()
const ui = useUi()
const score = computed(() => editor.report?.score)
const counts = computed(() => editor.report?.counts)
const rows = computed(() => (score.value?.breakdown ?? []).filter((b) => b.penalty > 0).sort((a, b) => b.penalty - a.penalty))
</script>

<template>
  <Modal :title="t('score.breakdown.title')" @close="ui.modal = null">
    <template v-if="score && counts">
      <p style="margin-top: 0">{{ t('score.breakdown.text', { c: counts.critical, r: counts.risk, n: counts.noData }) }}</p>
      <p class="muted">{{ t('score.breakdown.coverage', { pct: Math.round(score.coverage * 100) }) }}</p>
      <p v-if="!rows.length" class="muted">{{ t('score.breakdown.none') }}</p>
      <table v-else class="table">
        <tbody>
          <tr v-for="b in rows" :key="b.ruleId">
            <td class="mono nw">{{ b.ruleId }}</td>
            <td>{{ lt(app.rules.get(b.ruleId)?.title) }}</td>
            <td class="nw"><span class="tag" :class="statusTagClass(b.status)">{{ statusLabel(b.status) }}</span></td>
            <td class="mono right">−{{ Math.round(b.penalty * 10) / 10 }}</td>
          </tr>
        </tbody>
      </table>
    </template>
    <div v-else class="empty"><span class="spinner" /></div>
  </Modal>
</template>

<style scoped>
.nw { white-space: nowrap; }
</style>
