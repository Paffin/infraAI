<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { RefreshCw } from 'lucide-vue-next'
import * as api from '@/api/endpoints'
import { has, t } from '@/i18n'
import { errorMessage, fmtNumber } from '@/lib/format'
import type { SystemInfo } from '@/api/types'
import { jobTone, shortDbVersion, sortJobs, splitUptime } from '../lib/system'
import { kindPath } from '../lib/configKinds'
import StateBlock from '../components/StateBlock.vue'

const REFRESH_MS = 15000
const info = ref<SystemInfo | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)
const auto = ref(true)
const updatedAt = ref(0)
const now = ref(Date.now())
let poll: ReturnType<typeof setInterval> | undefined
let tick: ReturnType<typeof setInterval> | undefined

async function load() {
  loading.value = true
  try {
    info.value = await api.admin.system()
    updatedAt.value = Date.now()
    error.value = null
  } catch (e) {
    error.value = errorMessage(e)
  } finally {
    loading.value = false
  }
}

function visible() {
  return typeof document === 'undefined' || document.visibilityState === 'visible'
}

function onVisibility() {
  if (visible() && auto.value && Date.now() - updatedAt.value > REFRESH_MS) load()
}

onMounted(() => {
  load()
  poll = setInterval(() => { if (auto.value && visible() && !loading.value) load() }, REFRESH_MS)
  tick = setInterval(() => { now.value = Date.now() }, 1000)
  document.addEventListener('visibilitychange', onVisibility)
})
onBeforeUnmount(() => {
  clearInterval(poll)
  clearInterval(tick)
  document.removeEventListener('visibilitychange', onVisibility)
})

const ago = computed(() => (updatedAt.value ? Math.max(0, Math.round((now.value - updatedAt.value) / 1000)) : 0))
const uptime = computed(() => {
  const { d, h, m } = splitUptime(info.value?.uptimeSeconds ?? 0)
  const parts = d > 0 ? [t('common.days', { n: d }), t('common.hours', { n: h })] : h > 0 ? [t('common.hours', { n: h }), t('common.minutes', { n: m })] : [t('common.minutes', { n: m })]
  return parts.join(' ')
})
const jobs = computed(() => sortJobs(info.value?.jobsLast24h ?? []))
const jobTotal = computed(() => jobs.value.reduce((a, j) => a + j.count, 0))
const envRows = computed(() => Object.entries(info.value?.env ?? {}))

function jobLabel(s: string): string {
  return has(`admin.system.job.${s}`) ? t(`admin.system.job.${s}`) : s
}
function envText(v: string | number | boolean): string {
  return typeof v === 'boolean' ? (v ? t('common.yes') : t('common.no')) : String(v)
}
</script>

<template>
  <section class="adm-page">
    <header class="adm-head">
      <div class="grow">
        <h1>{{ t('admin.system.title') }}</h1>
        <p class="muted">{{ t('admin.system.subtitle') }}</p>
      </div>
      <label class="checkbox"><input v-model="auto" type="checkbox" /> {{ t('admin.system.auto') }}</label>
      <span v-if="updatedAt" class="faint nowrap" role="status">{{ t('admin.system.updated', { n: ago }) }}</span>
      <button class="btn" :disabled="loading" @click="load"><span v-if="loading" class="spinner" /><RefreshCw v-else :size="14" />{{ t('common.refresh') }}</button>
    </header>

    <StateBlock :loading="loading && !info" :error="error" @retry="load">
      <div v-if="error" class="banner banner-err" role="alert">{{ error }}</div>
      <div v-if="info" class="sys-grid">
        <div class="card card-pad sys-card">
          <h2>{{ t('admin.system.app') }}</h2>
          <dl class="kvlist">
            <div><dt>{{ t('admin.system.version') }}</dt><dd class="mono">{{ info.version }}</dd></div>
            <div><dt>{{ t('admin.system.engine') }}</dt><dd class="mono">{{ info.engineVersion }}</dd></div>
            <div><dt>{{ t('admin.system.instance') }}</dt><dd class="mono">{{ info.instance }}</dd></div>
            <div><dt>{{ t('admin.system.mode') }}</dt><dd><span class="tag" :class="info.production ? 'tag-ok' : 'tag-risk'">{{ info.production ? t('admin.system.production') : t('admin.system.development') }}</span></dd></div>
            <div><dt>{{ t('admin.system.uptime') }}</dt><dd>{{ uptime }}</dd></div>
            <div><dt>{{ t('admin.system.configRev') }}</dt><dd class="mono">{{ info.configRev }}</dd></div>
          </dl>
        </div>

        <div class="card card-pad sys-card">
          <h2>{{ t('admin.system.db') }}</h2>
          <dl class="kvlist">
            <div><dt>{{ t('admin.system.dbStatus') }}</dt><dd><span class="tag" :class="info.db.ok ? 'tag-ok' : 'tag-crit'">{{ info.db.ok ? t('admin.system.ok') : t('admin.system.down') }}</span></dd></div>
            <div><dt>{{ t('admin.system.dbLatency') }}</dt><dd>{{ t('admin.llm.ms', { n: fmtNumber(info.db.latencyMs) }) }}</dd></div>
            <div><dt>{{ t('admin.system.dbVersion') }}</dt><dd :title="info.db.version">{{ shortDbVersion(info.db.version) }}</dd></div>
            <div><dt>{{ t('admin.system.migrations') }}</dt><dd class="mono">{{ info.db.migrations }}</dd></div>
            <div><dt>{{ t('admin.system.pool') }}</dt><dd>{{ t('admin.system.poolValue', { size: info.db.poolSize, idle: info.db.poolIdle }) }}</dd></div>
          </dl>
        </div>

        <div class="card card-pad sys-card">
          <h2>{{ t('admin.system.jobs') }}</h2>
          <p v-if="!jobs.length" class="muted">{{ t('admin.system.noJobs') }}</p>
          <template v-else>
            <div class="job-total"><strong>{{ jobTotal }}</strong> <span class="muted">{{ t('admin.system.jobsTotal') }}</span></div>
            <div class="row wrap">
              <span v-for="j in jobs" :key="j.status" class="tag" :class="`tag-${jobTone(j.status)}`">{{ jobLabel(j.status) }} {{ j.count }}</span>
            </div>
          </template>
          <dl class="kvlist" style="margin-top: 14px">
            <div><dt>{{ t('admin.system.projects') }}</dt><dd>{{ fmtNumber(info.counts.projects) }}</dd></div>
            <div><dt>{{ t('admin.system.users') }}</dt><dd>{{ fmtNumber(info.counts.users) }}</dd></div>
          </dl>
        </div>

        <div class="card card-pad sys-card">
          <h2>{{ t('admin.system.llm') }}</h2>
          <dl class="kvlist">
            <div><dt>{{ t('admin.system.llmEnabled') }}</dt><dd><span class="tag" :class="info.llm.enabled ? 'tag-ok' : 'tag-nodata'">{{ info.llm.enabled ? t('common.yes') : t('common.no') }}</span></dd></div>
            <div><dt>{{ t('admin.system.llmKey') }}</dt><dd><span class="tag" :class="info.llm.keyConfigured ? 'tag-ok' : 'tag-crit'">{{ info.llm.keyConfigured ? t('admin.llm.keySet') : t('admin.llm.keyNone') }}</span></dd></div>
            <div><dt>{{ t('admin.system.llmSource') }}</dt><dd>{{ t(`admin.settings.secret.${info.llm.keySource === 'database' || info.llm.keySource === 'environment' ? info.llm.keySource : 'none'}`) }}</dd></div>
          </dl>
          <router-link to="/admin/llm" class="btn btn-sm" style="margin-top: 12px">{{ t('admin.system.openLlm') }}</router-link>
        </div>

        <div class="card card-pad sys-card sys-wide">
          <h2>{{ t('admin.system.env') }}</h2>
          <p class="muted env-note">{{ t('admin.system.envNote') }}</p>
          <dl class="kvlist env">
            <div v-for="[k, v] in envRows" :key="k">
              <dt class="mono">{{ k }}</dt>
              <dd><span v-if="typeof v === 'boolean'" class="tag" :class="v ? 'tag-ok' : 'tag-nodata'">{{ envText(v) }}</span><span v-else class="mono">{{ envText(v) }}</span></dd>
            </div>
          </dl>
        </div>

        <div class="card card-pad sys-card sys-wide">
          <h2>{{ t('admin.system.configErrors') }}</h2>
          <p v-if="!info.configErrors.length" class="muted">{{ t('admin.system.noConfigErrors') }}</p>
          <ul v-else class="err-list">
            <li v-for="e in info.configErrors" :key="e.kind + e.id">
              <router-link :to="kindPath(e.kind, e.id)" class="mono">{{ e.kind }} / {{ e.id }}</router-link>
              <span class="cfg-error-msg">{{ e.error }}</span>
            </li>
          </ul>
        </div>
      </div>
    </StateBlock>
  </section>
</template>
