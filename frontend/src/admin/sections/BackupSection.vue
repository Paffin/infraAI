<script setup lang="ts">
import { computed, ref } from 'vue'
import { ClipboardCheck, Download, FileUp, Play, ShieldCheck } from 'lucide-vue-next'
import * as api from '@/api/endpoints'
import { has, t } from '@/i18n'
import { errorMessage } from '@/lib/format'
import { useDialogs } from '@/stores/dialogs'
import { useToasts } from '@/stores/toasts'
import { exportFileName, parseImport } from '../lib/backup'
import { parseJson } from '../lib/json'
import { useAdminConfig } from '../store'
import JsonEditor from '../components/JsonEditor.vue'

interface DryRun { ok: boolean; problems: string[]; items: number; settings: number; applied: boolean }

const toasts = useToasts()
const dialogs = useDialogs()
const store = useAdminConfig()

const exporting = ref(false)
const exported = ref<{ file: string; items: number; settings: number } | null>(null)
const exportError = ref<string | null>(null)

const text = ref('')
const fileName = ref('')
const checking = ref(false)
const applying = ref(false)
const result = ref<DryRun | null>(null)
const checkedText = ref('')
const importError = ref<string | null>(null)
const fileInput = ref<HTMLInputElement | null>(null)

const parsedJson = computed(() => parseJson(text.value))
const parsed = computed(() => (text.value.trim() ? parseImport(text.value) : null))
const fresh = computed(() => !!result.value && checkedText.value === text.value)
const canApply = computed(() => fresh.value && result.value!.ok && !applying.value && !checking.value)

async function doExport() {
  exporting.value = true
  exportError.value = null
  try {
    const data = await api.admin.configExport()
    const file = exportFileName()
    const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = file
    document.body.appendChild(a)
    a.click()
    a.remove()
    setTimeout(() => URL.revokeObjectURL(url), 2000)
    exported.value = { file, items: Array.isArray(data?.items) ? data.items.length : 0, settings: Object.keys(data?.settings ?? {}).length }
    toasts.ok(t('admin.backup.exported', { file }))
  } catch (e) {
    exportError.value = errorMessage(e)
  } finally {
    exporting.value = false
  }
}

function onFile(e: Event) {
  const f = (e.target as HTMLInputElement).files?.[0]
  if (!f) return
  fileName.value = f.name
  const reader = new FileReader()
  reader.onload = () => { text.value = String(reader.result ?? ''); result.value = null; importError.value = null }
  reader.onerror = () => { importError.value = t('admin.backup.readFailed') }
  reader.readAsText(f)
  if (fileInput.value) fileInput.value.value = ''
}

async function check() {
  const p = parsed.value
  if (!p || !p.ok) return
  checking.value = true
  importError.value = null
  try {
    result.value = await api.admin.configImport({ ...p.payload, dryRun: true })
    checkedText.value = text.value
  } catch (e) {
    result.value = null
    importError.value = errorMessage(e)
  } finally {
    checking.value = false
  }
}

async function apply() {
  const p = parsed.value
  if (!p || !p.ok || !canApply.value) return
  const ok = await dialogs.confirm({
    title: t('admin.backup.applyTitle'),
    text: t('admin.backup.applyText', { items: result.value!.items, settings: result.value!.settings }),
    confirmLabel: t('admin.backup.apply'),
  })
  if (!ok) return
  applying.value = true
  importError.value = null
  try {
    const r = await api.admin.configImport({ ...p.payload, dryRun: false })
    result.value = r
    if (r.applied) {
      toasts.ok(t('admin.backup.applied', { items: r.items, settings: r.settings }))
      await store.load()
    }
  } catch (e) {
    importError.value = errorMessage(e)
    toasts.error(t('admin.backup.applyFailed'))
  } finally {
    applying.value = false
  }
}

function clear() {
  text.value = ''
  fileName.value = ''
  result.value = null
  importError.value = null
}

function kindLabel(kind: string): string {
  return has(`admin.kind.${kind}.title`) ? t(`admin.kind.${kind}.title`) : kind
}

const parseProblem = computed(() => {
  const p = parsed.value
  if (!p || p.ok) return ''
  return t(`admin.backup.bad.${p.reason}`)
})
</script>

<template>
  <section class="adm-page">
    <header class="adm-head">
      <div class="grow">
        <h1>{{ t('admin.backup.title') }}</h1>
        <p class="muted">{{ t('admin.backup.subtitle') }}</p>
      </div>
    </header>

    <div class="banner banner-info secrets-note"><ShieldCheck :size="16" /> <span>{{ t('admin.backup.secrets') }}</span></div>

    <div class="card card-pad">
      <div class="row sec-head">
        <h2 class="grow">{{ t('admin.backup.export') }}</h2>
        <button class="btn btn-primary" :disabled="exporting" @click="doExport"><span v-if="exporting" class="spinner" /><Download v-else :size="14" />{{ t('admin.backup.exportBtn') }}</button>
      </div>
      <p class="muted">{{ t('admin.backup.exportText') }}</p>
      <div v-if="exportError" class="banner banner-err" role="alert">{{ exportError }}</div>
      <div v-else-if="exported" class="banner banner-ok" role="status"><ClipboardCheck :size="15" /> {{ t('admin.backup.exportedInfo', { file: exported.file, items: exported.items, settings: exported.settings }) }}</div>
    </div>

    <div class="card card-pad">
      <div class="row sec-head">
        <h2 class="grow">{{ t('admin.backup.import') }}</h2>
        <input ref="fileInput" type="file" accept="application/json,.json" class="sr-only" tabindex="-1" aria-hidden="true" @change="onFile" />
        <button class="btn" @click="fileInput?.click()"><FileUp :size="14" />{{ t('admin.backup.pick') }}</button>
        <button v-if="text" class="btn btn-ghost" @click="clear">{{ t('admin.backup.clear') }}</button>
      </div>
      <p class="muted">{{ t('admin.backup.importText') }}</p>
      <div v-if="fileName" class="faint" style="margin-bottom: 6px">{{ t('admin.backup.file', { name: fileName }) }}</div>
      <JsonEditor v-model="text" :result="parsedJson" height="240px" :label="t('admin.backup.paste')" @update:model-value="result = null" />
      <p v-if="!text" class="hint">{{ t('admin.backup.pasteHint') }}</p>

      <div v-if="parseProblem" class="banner banner-warn" style="margin-top: 10px" role="status">{{ parseProblem }}</div>
      <div v-if="parsed?.ok" class="summary">
        <div class="kv">
          <div><span class="label">{{ t('admin.backup.sumItems') }}</span><strong>{{ parsed.summary.items }}</strong></div>
          <div><span class="label">{{ t('admin.backup.sumSettings') }}</span><strong>{{ parsed.summary.settings }}</strong></div>
          <div><span class="label">{{ t('admin.backup.sumFormat') }}</span><strong class="mono">{{ parsed.summary.format ?? '—' }}</strong></div>
        </div>
        <div v-if="!parsed.summary.formatOk" class="banner banner-warn" role="status" style="margin-top: 8px">{{ t('admin.backup.formatUnknown') }}</div>
        <div v-if="parsed.summary.byKind.length" class="row wrap" style="margin-top: 8px">
          <span v-for="[k, n] in parsed.summary.byKind" :key="k" class="chip">{{ kindLabel(k) }} <strong>{{ n }}</strong></span>
        </div>
      </div>

      <div class="row import-actions">
        <button class="btn" :disabled="!parsed?.ok || checking || applying" @click="check"><span v-if="checking" class="spinner" /><Play v-else :size="14" />{{ t('admin.backup.check') }}</button>
        <button class="btn btn-primary" :disabled="!canApply" @click="apply"><span v-if="applying" class="spinner" />{{ t('admin.backup.apply') }}</button>
        <span v-if="result && !fresh && !result.applied" class="faint">{{ t('admin.backup.stale') }}</span>
      </div>

      <div v-if="importError" class="banner banner-err" role="alert" style="margin-top: 10px">{{ importError }}</div>

      <div v-if="result" class="dry" role="status" aria-live="polite">
        <div v-if="result.applied" class="banner banner-ok"><ClipboardCheck :size="15" /> {{ t('admin.backup.applied', { items: result.items, settings: result.settings }) }}</div>
        <template v-else>
          <div v-if="result.ok" class="banner banner-ok"><ClipboardCheck :size="15" /> {{ t('admin.backup.dryOk', { items: result.items, settings: result.settings }) }}</div>
          <div v-else class="banner banner-err">
            <strong>{{ t('admin.backup.dryBad', { n: result.problems.length }) }}</strong>
            <ul class="problems"><li v-for="p in result.problems" :key="p" class="mono">{{ p }}</li></ul>
          </div>
        </template>
      </div>
    </div>
  </section>
</template>
