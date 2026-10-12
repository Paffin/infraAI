<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { CircleAlert, CircleCheck, CircleX, Plug, Zap } from 'lucide-vue-next'
import * as api from '@/api/endpoints'
import type { LlmModel, LlmTestResult as LlmTest } from '@/api/types'
import { has, t } from '@/i18n'
import { errorMessage, fmtDateTime, fmtNumber } from '@/lib/format'
import { useDirtyGuard } from '../lib/dirty'
import { fmtContext, fmtUsd, modelCaps } from '../lib/llm'
import { useSettingsEditor } from '../lib/useSettingsEditor'
import SettingControl from '../components/SettingControl.vue'
import SettingsSaveBar from '../components/SettingsSaveBar.vue'
import StateBlock from '../components/StateBlock.vue'
import LlmUsage from './LlmUsage.vue'

const QUICK = [
  'llm.enabled', 'llm.apiKey', 'llm.model.planner', 'llm.model.fast', 'llm.model.fallbacks', 'llm.structuredOutput',
  'llm.monthlyBudgetUsd', 'llm.perUserPerHour', 'llm.provider.dataCollection', 'llm.provider.zdr',
]

const ed = useSettingsEditor()
useDirtyGuard(() => ed.dirty.value.length > 0)

const usage = ref<InstanceType<typeof LlmUsage> | null>(null)
const result = ref<LlmTest | null>(null)
const testing = ref<'' | 'test' | 'ping'>('')
const testError = ref<string | null>(null)
const testedAt = ref<string | null>(null)
const catalog = ref<Record<string, LlmModel | null>>({})

const quick = computed(() => QUICK.map((k) => ed.byKey.value.get(k)).filter((d): d is NonNullable<typeof d> => !!d))
const apiKey = computed(() => ed.byKey.value.get('llm.apiKey'))
const keyState = computed(() => {
  if (result.value?.keySource && result.value.ok) return result.value.keySource
  return apiKey.value?.secretSource ?? 'none'
})

async function runTest(ping: boolean) {
  testing.value = ping ? 'ping' : 'test'
  testError.value = null
  try {
    const r = await api.admin.testLlm(ping)
    result.value = r
    testedAt.value = new Date().toISOString()
    await enrich(r)
  } catch (e) {
    testError.value = errorMessage(e)
  } finally {
    testing.value = ''
  }
}

/** The server reports found/structured/context per model; temperature support needs the catalog entry. */
async function enrich(r: LlmTest) {
  const out: Record<string, LlmModel | null> = {}
  await Promise.all(Object.values(r.models ?? {}).filter((m) => m.found).map(async (m) => {
    try {
      const res = await api.admin.models({ q: m.id, limit: 25 })
      out[m.id] = res.models.find((x) => x.id === m.id) ?? null
    } catch {
      out[m.id] = null
    }
  }))
  catalog.value = out
}

async function save() {
  const touched = ed.dirty.value.slice()
  const ok = await ed.save(touched)
  if (ok) {
    usage.value?.reload()
    runTest(false)
  }
}

function warningText(w: string): string {
  let m = /^model '(.+)' \((\w+)\) was not found/.exec(w)
  if (m) return t('admin.llm.warn.notFound', { model: m[1], role: roleName(m[2]) })
  m = /^model '(.+)' does not advertise structured outputs/.exec(w)
  if (m) return t('admin.llm.warn.noStructured', { model: m[1] })
  return w
}

function roleName(r: string): string {
  return has(`admin.llm.role.${r}`) ? t(`admin.llm.role.${r}`) : r
}

/** Known codes get a localised sentence, everything else is the server's message verbatim. */
function failureText(r: LlmTest): string {
  return pingCode(r.code) || r.message || t('admin.llm.failed')
}

function pingCode(code?: string): string {
  return code && has(`admin.llm.pingCode.${code}`) ? t(`admin.llm.pingCode.${code}`) : ''
}

const keyInfo = computed(() => result.value?.key ?? null)
const modelRows = computed(() => Object.entries(result.value?.models ?? {}).map(([role, m]) => {
  const c = catalog.value[m.id]
  return { role, ...m, caps: c ? modelCaps(c) : null }
}))

onMounted(async () => {
  await ed.load()
  runTest(false)
})
</script>

<template>
  <section class="adm-page">
    <header class="adm-head">
      <div class="grow">
        <h1>{{ t('admin.llm.title') }}</h1>
        <p class="muted">{{ t('admin.llm.subtitle') }}</p>
      </div>
    </header>

    <!-- connection -->
    <div class="card card-pad">
      <div class="row sec-head">
        <h2 class="grow">{{ t('admin.llm.connection') }}</h2>
        <button class="btn" :disabled="!!testing" @click="runTest(false)"><span v-if="testing === 'test'" class="spinner" /><Plug v-else :size="14" />{{ t('admin.llm.test') }}</button>
        <button class="btn" :disabled="!!testing || keyState === 'none'" :title="t('admin.llm.pingHint')" @click="runTest(true)"><span v-if="testing === 'ping'" class="spinner" /><Zap v-else :size="14" />{{ t('admin.llm.ping') }}</button>
      </div>

      <div class="row keystate">
        <span class="label">{{ t('admin.llm.key') }}</span>
        <span class="tag" :class="keyState === 'none' ? 'tag-crit' : 'tag-ok'">{{ keyState === 'none' ? t('admin.llm.keyNone') : t('admin.llm.keySet') }}</span>
        <span class="muted">{{ t(`admin.settings.secret.${keyState === 'database' || keyState === 'environment' ? keyState : 'none'}`) }}</span>
        <code v-if="keyInfo?.label" class="mono faint">{{ keyInfo.label }}</code>
        <span class="spacer" />
        <span v-if="testedAt" class="faint">{{ t('admin.llm.testedAt', { when: fmtDateTime(testedAt) }) }}</span>
      </div>

      <div v-if="testError" class="banner banner-err" role="alert">{{ testError }}</div>
      <template v-else-if="result">
        <div v-if="result.ok" class="banner banner-ok"><CircleCheck :size="15" /> {{ t('admin.llm.connected') }}</div>
        <div v-else class="banner banner-err" role="alert"><CircleX :size="15" /> {{ failureText(result) }}</div>

        <div v-if="keyInfo" class="kv">
          <div><span class="label">{{ t('admin.llm.limit') }}</span><strong>{{ keyInfo.limit != null ? fmtUsd(keyInfo.limit) : t('admin.llm.unlimited') }}</strong></div>
          <div v-if="keyInfo.limit != null"><span class="label">{{ t('admin.llm.remaining') }}</span><strong>{{ fmtUsd(keyInfo.limitRemaining) }}</strong></div>
          <div><span class="label">{{ t('admin.llm.spent') }}</span><strong>{{ fmtUsd(keyInfo.usage) }}</strong></div>
          <div><span class="label">{{ t('admin.llm.tier') }}</span><strong>{{ keyInfo.isFreeTier ? t('admin.llm.free') : t('admin.llm.paid') }}</strong></div>
        </div>

        <template v-if="modelRows.length">
          <h3 class="sec-sub">{{ t('admin.llm.models') }}</h3>
          <div class="tablewrap">
            <table class="table">
              <thead>
                <tr>
                  <th>{{ t('admin.llm.colRole') }}</th><th>{{ t('admin.llm.colModel') }}</th><th>{{ t('admin.llm.colFound') }}</th>
                  <th>{{ t('admin.llm.colStructured') }}</th><th>{{ t('admin.llm.colTemperature') }}</th><th class="num">{{ t('admin.llm.colContext') }}</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="m in modelRows" :key="m.role">
                  <td>{{ roleName(m.role) }}</td>
                  <td class="mono">{{ m.id }}</td>
                  <td><span class="tag" :class="m.found ? 'tag-ok' : 'tag-crit'">{{ m.found ? t('common.yes') : t('common.no') }}</span></td>
                  <td><span v-if="m.found" class="tag" :class="m.structuredOutputs ? 'tag-ok' : 'tag-risk'">{{ m.structuredOutputs ? t('common.yes') : t('common.no') }}</span><span v-else class="faint">—</span></td>
                  <td>
                    <span v-if="m.caps" class="tag" :class="m.caps.temperature ? 'tag-ok' : 'tag-nodata'">{{ m.caps.temperature ? t('common.yes') : t('admin.llm.omitted') }}</span>
                    <span v-else class="faint">—</span>
                  </td>
                  <td class="num">{{ fmtContext(m.contextLength) }}</td>
                </tr>
              </tbody>
            </table>
          </div>
        </template>

        <div v-if="result.warnings?.length" class="banner banner-warn warnings" role="status">
          <CircleAlert :size="15" />
          <ul><li v-for="w in result.warnings" :key="w">{{ warningText(w) }}</li></ul>
        </div>

        <div v-if="result.ping" class="ping" :class="result.ping.ok ? 'ok' : 'bad'">
          <h3 class="sec-sub">{{ t('admin.llm.pingResult') }}</h3>
          <template v-if="result.ping.ok">
            <div class="kv">
              <div><span class="label">{{ t('admin.llm.pingModel') }}</span><strong class="mono">{{ result.ping.model }}</strong></div>
              <div><span class="label">{{ t('admin.llm.pingLatency') }}</span><strong>{{ t('admin.llm.ms', { n: fmtNumber(result.ping.latencyMs) }) }}</strong></div>
              <div><span class="label">{{ t('admin.llm.pingCost') }}</span><strong>{{ result.ping.costUsd != null ? fmtUsd(result.ping.costUsd) : '—' }}</strong></div>
              <div><span class="label">{{ t('admin.llm.pingAnswer') }}</span><strong class="mono">{{ result.ping.answer }}</strong></div>
            </div>
          </template>
          <div v-else class="banner banner-err" role="alert">
            <strong v-if="pingCode(result.ping.code)">{{ pingCode(result.ping.code) }}. </strong>{{ result.ping.message }}
            <code v-if="result.ping.code" class="faint"> ({{ result.ping.code }})</code>
          </div>
        </div>
      </template>
      <StateBlock v-else :loading="!!testing" />
    </div>

    <!-- key settings, edited with the same controls as on the Settings page -->
    <div class="card srows">
      <div class="row sec-head srows-head">
        <h2 class="grow">{{ t('admin.llm.quick') }}</h2>
        <router-link to="/admin/settings/llm" class="btn btn-sm">{{ t('admin.llm.allSettings') }}</router-link>
      </div>
      <StateBlock :loading="ed.loading.value && !ed.defs.value.length" :error="ed.loadError.value" @retry="ed.load()">
        <SettingControl
          v-for="d in quick" :key="d.key" :def="d" :model-value="ed.drafts[d.key]" :error="ed.errors[d.key]" :disabled="ed.saving.value"
          @update:model-value="ed.setDraft(d.key, $event)" @reset="ed.resetToDefault(d.key)"
        />
      </StateBlock>
    </div>

    <LlmUsage ref="usage" />

    <SettingsSaveBar :count="ed.dirty.value.length" :saving="ed.saving.value" :status="ed.status.value" @save="save" @discard="ed.discard()" />
  </section>
</template>
