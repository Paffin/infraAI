<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { Upload } from 'lucide-vue-next'
import * as api from '@/api/endpoints'
import type { ArchDiff, Architecture, VersionItem } from '@/api/types'
import Modal from '@/components/Modal.vue'
import { lang, t } from '@/i18n'
import { diffIsEmpty, diffLines } from '@/lib/diff'
import { errorMessage, fmtDateTime } from '@/lib/format'
import { useApp } from '@/stores/app'
import { useDialogs } from '@/stores/dialogs'
import { useEditor } from '@/stores/editor'
import { useToasts } from '@/stores/toasts'
import { useUi } from '@/stores/ui'

const app = useApp()
const editor = useEditor()
const ui = useUi()
const dialogs = useDialogs()
const toasts = useToasts()

const versions = ref<VersionItem[]>([])
const loading = ref(true)
const message = ref('')
const from = ref<string>('draft')
const to = ref<string>('draft')
const diff = ref<ArchDiff | null>(null)
const diffBusy = ref(false)
const busy = ref(false)
const canWrite = computed(() => app.canEdit && !editor.readonly)

// drift: the design against an imported snapshot of the real environment
const actual = ref<{ name: string; arch: Architecture } | null>(null)
const driftDiff = ref<ArchDiff | null>(null)
const driftError = ref('')
const file = ref<HTMLInputElement | null>(null)

async function loadFile(e: Event) {
  const f = (e.target as HTMLInputElement).files?.[0]
  ;(e.target as HTMLInputElement).value = ''
  if (!f) return
  driftError.value = ''
  try {
    let j = JSON.parse(await f.text())
    if (j && typeof j === 'object' && 'architecture' in j) j = j.architecture
    if (!j || !Array.isArray(j.nodes) || !Array.isArray(j.edges)) throw new Error(t('versions.drift.invalid'))
    j = { schemaVersion: 1, meta: {}, requirements: {}, zones: [], ...j }
    driftDiff.value = await api.engine.diff(editor.arch, j as Architecture)
    actual.value = { name: f.name, arch: j as Architecture }
    diff.value = null
  } catch (err) {
    actual.value = null
    driftDiff.value = null
    driftError.value = errorMessage(err)
  }
}

function showOnCanvas() {
  const d = actual.value ? driftDiff.value : diff.value
  if (!d) return
  const label = actual.value ? t('versions.drift.label', { name: actual.value.name }) : `${label_(from.value)} → ${label_(to.value)}`
  ui.modal = null
  Promise.all([actual.value ? Promise.resolve(editor.arch) : arch(from.value), actual.value ? Promise.resolve(actual.value.arch) : arch(to.value)]).then(([base, next]) => editor.setPreview({ kind: 'compare', label, proposalId: '', base, arch: next, diff: d }))
}

async function arch(v: string): Promise<Architecture> {
  return v === 'draft' ? editor.arch : (await api.projects.version(editor.projectId, Number(v))).architecture
}

async function adoptActual() {
  if (!actual.value || !canWrite.value) return
  if (!(await dialogs.confirm({ title: t('versions.drift.accept'), text: t('versions.drift.accept.confirm'), confirmLabel: t('versions.drift.accept'), danger: true }))) return
  editor.replaceArchitecture(actual.value.arch)
  ui.modal = null
}

async function load() {
  loading.value = true
  try {
    await editor.flushSave()
    versions.value = (await api.projects.versions(editor.projectId)).versions
    if (versions.value.length) { from.value = String(versions.value[0].n); to.value = 'draft' }
  } catch (e) {
    toasts.error(errorMessage(e))
  } finally {
    loading.value = false
  }
}

async function compare() {
  if (from.value === to.value) { diff.value = null; return }
  diffBusy.value = true
  try {
    diff.value = await api.projects.diff(editor.projectId, from.value, to.value)
  } catch (e) {
    diff.value = null
    toasts.error(errorMessage(e))
  } finally {
    diffBusy.value = false
  }
}

async function save() {
  busy.value = true
  try {
    await editor.flushSave()
    await api.projects.createVersion(editor.projectId, message.value.trim() || t('versions.default'), lang.value)
    message.value = ''
    const p = await api.projects.get(editor.projectId)
    editor.project = p.project
    await load()
    toasts.ok(t('saved.toast'))
  } catch (e) {
    toasts.error(errorMessage(e))
  } finally {
    busy.value = false
  }
}

async function restore(v: VersionItem) {
  if (!(await dialogs.confirm({ title: t('versions.restore'), text: t('versions.restore.confirm', { n: v.n }), confirmLabel: t('versions.restore'), danger: true }))) return
  busy.value = true
  try {
    await editor.flushSave()
    await api.projects.restore(editor.projectId, v.n, editor.rev)
    await editor.reloadFromServer()
    ui.modal = null
  } catch (e) {
    toasts.error(errorMessage(e))
  } finally {
    busy.value = false
  }
}

const shownDiff = computed(() => (actual.value ? driftDiff.value : diff.value))
const lines = computed(() => (shownDiff.value ? diffLines(shownDiff.value, 6) : []))
const label_ = (v: string) => (v === 'draft' ? t('versions.draft') : `v${v}`)
onMounted(load)
watch([from, to], () => { actual.value = null; driftDiff.value = null; void compare() })
watch(versions, compare)
</script>

<template>
  <Modal :title="t('versions.title')" size="xl" @close="ui.modal = null">
    <div class="vers">
      <section class="left">
        <form v-if="canWrite" class="row save" @submit.prevent="save">
          <input v-model="message" class="input" :placeholder="t('versions.message')" maxlength="200" />
          <button class="btn btn-primary" :disabled="busy">{{ t('versions.save') }}</button>
        </form>
        <div v-if="loading" class="empty"><span class="spinner" /></div>
        <p v-else-if="!versions.length" class="muted">{{ t('versions.empty') }}</p>
        <ul v-else class="list">
          <li v-for="v in versions" :key="v.n">
            <div class="top"><b>v{{ v.n }}</b><span class="grow truncate">{{ v.message }}</span><span class="tag tag-mono tag-accent">{{ t('versions.score', { v: Math.round(v.summary.score) }) }}</span></div>
            <div class="meta faint">{{ v.author }} · {{ fmtDateTime(v.createdAt) }}</div>
            <div class="acts">
              <button class="btn btn-sm" @click="from = String(v.n)">{{ t('versions.from') }}</button>
              <button class="btn btn-sm" @click="to = String(v.n)">{{ t('versions.to') }}</button>
              <button v-if="canWrite" class="btn btn-sm btn-ghost" :disabled="busy" @click="restore(v)">{{ t('versions.restore') }}</button>
            </div>
          </li>
        </ul>
        <div class="drift">
          <h5>{{ t('versions.drift.title') }}</h5>
          <p class="muted small">{{ t('versions.drift.hint') }}</p>
          <input ref="file" type="file" accept="application/json,.json" class="sr-only" @change="loadFile" />
          <button class="btn btn-sm" @click="file?.click()"><Upload :size="13" />{{ t('versions.drift.load') }}</button>
          <p v-if="driftError" class="banner banner-err small">{{ driftError }}</p>
        </div>
      </section>
      <section class="right">
        <div class="row pick">
          <span class="muted">{{ t('versions.compare') }}</span>
          <select v-model="from" class="select"><option value="draft">{{ t('versions.draft') }}</option><option v-for="v in versions" :key="v.n" :value="String(v.n)">v{{ v.n }}</option></select>
          <span>→</span>
          <select v-model="to" class="select"><option value="draft">{{ t('versions.draft') }}</option><option v-for="v in versions" :key="v.n" :value="String(v.n)">v{{ v.n }}</option></select>
        </div>
        <p v-if="actual" class="muted small"><b>{{ t('versions.drift.result', { name: actual.name }) }}</b></p>
        <div v-if="diffBusy" class="empty"><span class="spinner" /></div>
        <p v-else-if="!actual && from === to" class="muted">{{ label_(from) }} = {{ label_(to) }}</p>
        <p v-else-if="shownDiff && diffIsEmpty(shownDiff)" class="muted">{{ actual ? t('versions.drift.none') : t('diff.nothing') }}</p>
        <div v-else-if="shownDiff" class="difflist mono">
          <div v-for="(l, i) in lines" :key="i" class="ln" :class="l.sign === '+' ? 'add' : l.sign === '-' ? 'del' : 'chg'"><span class="sg">{{ l.sign === '-' ? '−' : l.sign }}</span><span class="grp">{{ t('diff.' + l.group) }}</span><span class="tx">{{ l.text }}</span></div>
        </div>
        <div v-if="shownDiff && !diffIsEmpty(shownDiff)" class="row diffact">
          <button class="btn btn-sm" @click="showOnCanvas">{{ t('proposal.preview') }}</button>
          <button v-if="actual && canWrite" class="btn btn-sm btn-danger" @click="adoptActual">{{ t('versions.drift.accept') }}</button>
        </div>
      </section>
    </div>
  </Modal>
</template>

<style scoped>
.vers { display: grid; grid-template-columns: minmax(320px, 1fr) minmax(0, 1.3fr); gap: 24px; min-height: 360px; }
.save { margin-bottom: 14px; }
.list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 10px; max-height: 52vh; overflow: auto; }
.list li { border: 1px solid var(--line-strong); border-radius: var(--radius); padding: 10px 12px; }
.top { display: flex; align-items: center; gap: 8px; }
.meta { font-size: 12px; margin: 2px 0 8px; }
.acts { display: flex; gap: 6px; }
.pick { margin-bottom: 14px; }
.drift { margin-top: 18px; padding-top: 14px; border-top: 1px solid var(--line); display: flex; flex-direction: column; gap: 8px; align-items: flex-start; }
.drift h5 { margin: 0; font-size: 11px; letter-spacing: .09em; text-transform: uppercase; color: #6e7fa8; font-weight: 600; }
.small { font-size: 12.5px; margin: 0; }
.diffact { margin-top: 12px; }
.pick .select { width: 190px; }
.difflist { border: 1px solid var(--line); border-radius: var(--radius); background: var(--surface-2); padding: 10px 12px; font-size: 12px; line-height: 1.6; max-height: 52vh; overflow: auto; }
.ln { display: flex; gap: 8px; }
.sg { width: 10px; flex: none; }
.grp { color: var(--faint); width: 74px; flex: none; }
.tx { overflow-wrap: anywhere; }
.add { color: var(--ok); } .del { color: var(--crit); } .chg { color: var(--risk); }
@media (max-width: 900px) { .vers { grid-template-columns: 1fr; } }
</style>
