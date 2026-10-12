<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { RefreshCw } from 'lucide-vue-next'
import * as api from '@/api/endpoints'
import { t } from '@/i18n'
import { errorMessage, fmtNumber } from '@/lib/format'
import { budgetRatio, fillDays, fmtUsd, usageTotals, type Usage } from '../lib/llm'
import StateBlock from '../components/StateBlock.vue'
import UsageChart from '../components/UsageChart.vue'

const days = ref(30)
const data = ref<Usage | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)
const showTable = ref(false)

async function load() {
  loading.value = true
  try {
    data.value = await api.admin.usage(days.value)
    error.value = null
  } catch (e) {
    error.value = errorMessage(e)
  } finally {
    loading.value = false
  }
}
onMounted(load)
watch(days, load)
defineExpose({ reload: load })

const totals = computed(() => usageTotals(data.value?.daily ?? []))
const filled = computed(() => fillDays(data.value?.daily ?? [], days.value))
const ratio = computed(() => (data.value ? budgetRatio(data.value.monthSpendUsd, data.value.monthlyBudgetUsd) : null))
const pct = computed(() => Math.min(100, Math.round((ratio.value ?? 0) * 100)))
const level = computed(() => ((ratio.value ?? 0) >= 1 ? 'crit' : (ratio.value ?? 0) >= 0.8 ? 'risk' : 'ok'))
const empty = computed(() => !!data.value && !data.value.daily.length)
</script>

<template>
  <div class="card card-pad usage">
    <div class="row sec-head">
      <h2 class="grow">{{ t('admin.llm.usage') }}</h2>
      <label class="sr-only" for="usage-days">{{ t('admin.llm.period') }}</label>
      <select id="usage-days" v-model.number="days" class="select input-sm" style="width: auto">
        <option :value="7">{{ t('admin.llm.days', { n: 7 }) }}</option>
        <option :value="30">{{ t('admin.llm.days', { n: 30 }) }}</option>
        <option :value="90">{{ t('admin.llm.days', { n: 90 }) }}</option>
      </select>
      <button class="btn btn-icon btn-sm" :title="t('common.refresh')" :aria-label="t('common.refresh')" :disabled="loading" @click="load"><RefreshCw :size="14" /></button>
    </div>

    <StateBlock :loading="loading && !data" :error="error" @retry="load">
      <template v-if="data">
        <div class="budget">
          <div class="row">
            <span class="label">{{ t('admin.llm.monthSpend') }}</span>
            <span class="spacer" />
            <strong>{{ fmtUsd(data.monthSpendUsd) }}</strong>
            <span class="muted">{{ data.monthlyBudgetUsd > 0 ? t('admin.llm.ofBudget', { budget: fmtUsd(data.monthlyBudgetUsd) }) : t('admin.llm.noBudget') }}</span>
          </div>
          <div v-if="ratio !== null" class="progress" role="progressbar" :aria-valuenow="pct" aria-valuemin="0" aria-valuemax="100" :aria-label="t('admin.llm.monthSpend')">
            <div class="progress-fill" :class="level" :style="{ width: pct + '%' }" />
          </div>
          <div v-if="ratio !== null && ratio >= 1" class="banner banner-err" role="alert" style="margin-top: 8px">{{ t('admin.llm.budgetExhausted') }}</div>
          <div v-else-if="ratio !== null && ratio >= 0.8" class="banner banner-warn" style="margin-top: 8px">{{ t('admin.llm.budgetNear', { pct }) }}</div>
        </div>

        <div class="tiles">
          <div class="tile"><span class="label">{{ t('admin.llm.tileCalls') }}</span><strong>{{ fmtNumber(totals.calls) }}</strong></div>
          <div class="tile"><span class="label">{{ t('admin.llm.tileErrors') }}</span><strong>{{ fmtNumber(totals.errors) }}</strong></div>
          <div class="tile"><span class="label">{{ t('admin.llm.tileTokens') }}</span><strong>{{ fmtNumber(totals.tokens) }}</strong></div>
          <div class="tile"><span class="label">{{ t('admin.llm.tileCost', { n: days }) }}</span><strong>{{ fmtUsd(totals.cost) }}</strong></div>
        </div>

        <div v-if="empty" class="empty">{{ t('admin.llm.usageEmpty') }}</div>
        <template v-else>
          <div class="row sec-sub">
            <h3 class="grow">{{ t('admin.llm.perDay') }}</h3>
            <button class="btn btn-sm btn-ghost" @click="showTable = !showTable">{{ showTable ? t('admin.llm.showChart') : t('admin.llm.showTable') }}</button>
          </div>
          <UsageChart v-if="!showTable" :days="filled" />
          <div v-else class="tablewrap" style="max-height: 280px">
            <table class="table">
              <thead><tr><th>{{ t('admin.llm.colDay') }}</th><th class="num">{{ t('admin.llm.colCalls') }}</th><th class="num">{{ t('admin.llm.colErrors') }}</th><th class="num">{{ t('admin.llm.colTokens') }}</th><th class="num">{{ t('admin.llm.colCost') }}</th></tr></thead>
              <tbody>
                <tr v-for="d in [...data.daily].reverse()" :key="d.day">
                  <td>{{ d.day }}</td><td class="num">{{ fmtNumber(d.calls) }}</td><td class="num">{{ fmtNumber(d.errors) }}</td>
                  <td class="num">{{ fmtNumber(d.promptTokens + d.completionTokens) }}</td><td class="num">{{ fmtUsd(d.costUsd) }}</td>
                </tr>
              </tbody>
            </table>
          </div>

          <div class="usage-tables">
            <div>
              <h3 class="sec-sub">{{ t('admin.llm.perModel') }}</h3>
              <div class="tablewrap">
                <table class="table">
                  <thead><tr><th>{{ t('admin.llm.colModel') }}</th><th class="num">{{ t('admin.llm.colCalls') }}</th><th class="num">{{ t('admin.llm.colLatency') }}</th><th class="num">{{ t('admin.llm.colCost') }}</th></tr></thead>
                  <tbody>
                    <tr v-for="m in data.byModel" :key="m.model">
                      <td class="mono">{{ m.model }}</td><td class="num">{{ fmtNumber(m.calls) }}</td>
                      <td class="num">{{ t('admin.llm.ms', { n: fmtNumber(m.avgLatencyMs) }) }}</td><td class="num">{{ fmtUsd(m.costUsd) }}</td>
                    </tr>
                  </tbody>
                </table>
              </div>
            </div>
            <div>
              <h3 class="sec-sub">{{ t('admin.llm.perUser') }}</h3>
              <div class="tablewrap">
                <table class="table">
                  <thead><tr><th>{{ t('admin.llm.colUser') }}</th><th class="num">{{ t('admin.llm.colCalls') }}</th><th class="num">{{ t('admin.llm.colCost') }}</th></tr></thead>
                  <tbody>
                    <tr v-for="u in data.byUser" :key="u.user"><td>{{ u.user }}</td><td class="num">{{ fmtNumber(u.calls) }}</td><td class="num">{{ fmtUsd(u.costUsd) }}</td></tr>
                  </tbody>
                </table>
              </div>
            </div>
          </div>
        </template>
      </template>
    </StateBlock>
  </div>
</template>
