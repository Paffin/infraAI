<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Anchor, Braces, CircleCheck, Container, Copy, Crosshair, Download, FileText, Info, KeyRound, Layers, ListChecks, TriangleAlert, Workflow } from 'lucide-vue-next'
import * as api from '@/api/endpoints'
import type { ExportFile, ExportFormatInfo, IacReport, IacSkipped } from '@/api/types'
import Modal from '@/components/Modal.vue'
import { has, lang, t } from '@/i18n'
import { errorMessage } from '@/lib/format'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'
import { useToasts } from '@/stores/toasts'
import { useUi } from '@/stores/ui'

type Tab = 'coverage' | 'setup' | 'notes' | 'files'

const app = useApp()
const editor = useEditor()
const ui = useUi()
const toasts = useToasts()

const ICONS: Record<string, unknown> = { kubernetes: Container, helm: Anchor, terraform: Layers, adr: FileText, runbook: ListChecks, mermaid: Workflow, json: Braces }
const GROUP_ORDER = ['iac', 'docs', 'data']

const formats = computed<ExportFormatInfo[]>(() => app.boot?.exportFormats ?? [])
const groups = computed(() =>
  GROUP_ORDER.map((g) => ({ id: g, items: formats.value.filter((f) => f.group === g) })).filter((g) => g.items.length),
)
const format = ref(formats.value.find((f) => f.id === 'terraform')?.id ?? formats.value[0]?.id ?? 'json')
const files = ref<ExportFile[]>([])
const report = ref<IacReport | null>(null)
const current = ref(0)
const tab = ref<Tab>('files')
const loading = ref(false)
const error = ref('')
const copied = ref(false)

async function load() {
  loading.value = true
  error.value = ''
  try {
    await editor.flushSave()
    const r = await api.projects.export(editor.projectId, format.value, lang.value)
    files.value = r.files
    report.value = r.report
    current.value = 0
    tab.value = r.report ? 'coverage' : 'files'
  } catch (e) {
    files.value = []
    report.value = null
    error.value = errorMessage(e)
  } finally {
    loading.value = false
  }
}

async function copy() {
  const f = files.value[current.value]
  if (!f) return
  try {
    await navigator.clipboard.writeText(f.content)
    copied.value = true
    setTimeout(() => (copied.value = false), 1500)
  } catch {
    toasts.error(t('error.generic', { message: 'clipboard' }))
  }
}

const href = computed(() => api.projects.exportUrl(editor.projectId, format.value, lang.value))
const content = computed(() => files.value[current.value]?.content ?? '')

// ------------------------------------------------------------------------------- IaC report
const stats = computed(() => {
  const r = report.value
  if (!r) return null
  const done = new Set(r.generated.map((g) => g.unit))
  const open = new Set(r.skipped.map((s) => s.unit))
  const total = new Set([...done, ...open]).size
  return {
    generated: done.size,
    total,
    skipped: r.skipped.length,
    warnings: r.notes.filter((n) => n.level === 'warn').length,
    clusters: r.clusters.length,
    pct: total ? Math.round((done.size / total) * 100) : 0,
  }
})

/** Reasons the user can resolve in the diagram come first; "no generator" is information. */
const SKIP_ORDER = ['needs_input', 'no_cluster', 'hosting', 'unsupported', 'template_error', 'no_workload', 'disabled', 'no_template', 'no_generator']
const skipped = computed(() => [...(report.value?.skipped ?? [])].sort((a, b) => SKIP_ORDER.indexOf(a.code) - SKIP_ORDER.indexOf(b.code)))
const actionable = (s: IacSkipped) => ['needs_input', 'no_cluster', 'hosting', 'unsupported'].includes(s.code)
const skipLabel = (code: string) => (has('export.skip.' + code) ? t('export.skip.' + code) : code)
const skipTone = (code: string) => (code === 'template_error' ? 'tag-crit' : actionable({ code } as IacSkipped) ? 'tag-risk' : 'tag-nodata')

function show(s: IacSkipped) {
  const id = s.nodes[0]
  if (!id) return
  ui.modal = null
  editor.select({ type: 'node', id })
}

const setupCount = computed(() => (report.value ? report.value.prerequisites.length + report.value.secrets.length : 0))
const tabs = computed(() => {
  const list: { id: Tab; label: string; badge?: number }[] = []
  if (report.value) {
    list.push({ id: 'coverage', label: t('export.tab.coverage'), badge: report.value.skipped.length || undefined })
    list.push({ id: 'setup', label: t('export.tab.setup'), badge: setupCount.value || undefined })
    list.push({ id: 'notes', label: t('export.tab.notes'), badge: report.value.notes.length || undefined })
  }
  list.push({ id: 'files', label: t('export.tab.files'), badge: files.value.length })
  return list
})

watch(format, load, { immediate: true })
</script>

<template>
  <Modal :title="t('export.title')" size="xl" @close="ui.modal = null">
    <div class="exp">
      <nav class="formats" :aria-label="t('export.title')">
        <section v-for="g in groups" :key="g.id" class="grp">
          <h4>{{ t('export.group.' + g.id) }}</h4>
          <button v-for="f in g.items" :key="f.id" :class="{ on: format === f.id }" :aria-pressed="format === f.id" @click="format = f.id">
            <component :is="ICONS[f.id] ?? FileText" :size="16" class="ico" />
            <span class="txt">
              <b>{{ t('export.' + f.id) }}</b>
              <small>{{ t('export.' + f.id + '.desc') }}</small>
            </span>
          </button>
        </section>
      </nav>

      <section class="main">
        <div v-if="loading" class="empty"><span class="spinner" /></div>
        <div v-else-if="error" class="banner banner-err">{{ error }}</div>
        <template v-else>
          <div v-if="stats" class="summary" data-test="export-summary">
            <div class="stat">
              <b>{{ stats.generated }}<span class="of"> / {{ stats.total }}</span></b>
              <small>{{ t('export.summary.generated') }}</small>
              <i class="meter"><u :style="{ width: stats.pct + '%' }" /></i>
            </div>
            <div class="stat" :class="{ warn: stats.skipped }"><b>{{ stats.skipped }}</b><small>{{ t('export.summary.skipped') }}</small></div>
            <div class="stat" :class="{ warn: stats.warnings }"><b>{{ stats.warnings }}</b><small>{{ t('export.summary.warnings') }}</small></div>
            <div class="stat"><b>{{ stats.clusters }}</b><small>{{ t('export.summary.clusters') }}</small></div>
          </div>
          <p v-if="report" class="honest muted"><Info :size="14" />{{ t('export.honest') }}</p>

          <div class="tabsbar" role="tablist">
            <button v-for="x in tabs" :key="x.id" role="tab" :aria-selected="tab === x.id" :class="{ on: tab === x.id }" :data-test="'export-tab-' + x.id" @click="tab = x.id">
              {{ x.label }}<span v-if="x.badge" class="bdg">{{ x.badge }}</span>
            </button>
          </div>

          <!-- coverage -->
          <div v-if="tab === 'coverage' && report" class="pane">
            <div v-if="!report.clusters.length" class="banner banner-warn">{{ t('export.clusters.none') }}</div>
            <h5 v-if="report.generated.length">{{ t('export.generated.title') }}</h5>
            <ul class="rows">
              <li v-for="(g, i) in report.generated" :key="g.unit + g.cluster + i" class="row-g">
                <CircleCheck :size="16" class="okc" />
                <div class="body">
                  <div class="head"><b>{{ g.name }}</b> <span class="tag tag-mono">{{ g.kind }}</span> <span class="muted">→ {{ g.cluster }}</span></div>
                  <div class="items"><code v-for="it in g.items" :key="it">{{ it }}</code></div>
                </div>
              </li>
            </ul>
            <h5 v-if="skipped.length">{{ t('export.skipped.title') }}</h5>
            <ul class="rows">
              <li v-for="(s, i) in skipped" :key="s.unit + s.code + i" class="row-s" :data-test="'export-skipped-' + s.code">
                <div class="body">
                  <div class="head">
                    <b>{{ s.name }}</b> <span class="tag tag-mono">{{ s.kind }}</span>
                    <span class="tag" :class="skipTone(s.code)">{{ skipLabel(s.code) }}</span>
                  </div>
                  <p class="why">{{ s.message }}</p>
                </div>
                <button v-if="s.nodes.length && actionable(s)" class="btn btn-sm" @click="show(s)"><Crosshair :size="13" />{{ t('export.show') }}</button>
              </li>
            </ul>
            <p v-if="!skipped.length && report.generated.length" class="muted">{{ t('export.all.done') }}</p>
          </div>

          <!-- prerequisites and secrets -->
          <div v-else-if="tab === 'setup' && report" class="pane">
            <h5>{{ t('export.prereq.title') }}</h5>
            <p class="muted small">{{ t('export.prereq.text') }}</p>
            <table v-if="report.prerequisites.length" class="table">
              <thead><tr><th>{{ t('export.cluster') }}</th><th>{{ t('export.component') }}</th><th>{{ t('export.chart') }}</th><th>{{ t('export.version') }}</th></tr></thead>
              <tbody>
                <tr v-for="p in report.prerequisites" :key="p.cluster + p.id">
                  <td>{{ p.cluster }}</td><td>{{ p.title }}</td><td><code>{{ p.repo }}/{{ p.chart }}</code></td><td><code>{{ p.version }}</code></td>
                </tr>
              </tbody>
            </table>
            <p v-else class="muted">{{ t('export.none') }}</p>
            <h5><KeyRound :size="14" /> {{ t('export.secrets.title') }}</h5>
            <p class="muted small">{{ t('export.secrets.text') }}</p>
            <table v-if="report.secrets.length" class="table">
              <thead><tr><th>{{ t('export.cluster') }}</th><th>{{ t('export.namespace') }}</th><th>{{ t('export.secret') }}</th><th>{{ t('export.keys') }}</th></tr></thead>
              <tbody>
                <tr v-for="s in report.secrets" :key="s.cluster + s.name">
                  <td>{{ s.cluster }}</td><td>{{ s.namespace }}</td>
                  <td><code>{{ s.name }}</code><div v-if="s.hint" class="muted small">{{ s.hint }}</div></td>
                  <td><code v-for="k in s.keys" :key="k" class="key">{{ k }}</code></td>
                </tr>
              </tbody>
            </table>
            <p v-else class="muted">{{ t('export.none') }}</p>
          </div>

          <!-- notes -->
          <div v-else-if="tab === 'notes' && report" class="pane">
            <ul v-if="report.notes.length" class="rows">
              <li v-for="(n, i) in report.notes" :key="i" class="row-n" :class="n.level">
                <TriangleAlert v-if="n.level === 'warn'" :size="16" class="wc" />
                <Info v-else :size="16" class="ic" />
                <div class="body"><p class="why">{{ n.message }}</p><small v-if="n.unit" class="muted">{{ n.unit }}</small></div>
              </li>
            </ul>
            <p v-else class="muted">{{ t('export.notes.none') }}</p>
          </div>

          <!-- files -->
          <template v-else>
            <div class="bar row">
              <div class="tabs2 grow">
                <button v-for="(f, i) in files" :key="f.path" :class="{ on: i === current }" :title="f.path" @click="current = i">{{ f.path }}</button>
              </div>
              <button class="btn btn-sm" @click="copy"><Copy :size="13" />{{ copied ? t('export.copied') : t('export.copy') }}</button>
              <a class="btn btn-sm btn-primary" :href="href" download><Download :size="13" />{{ t('export.download') }}</a>
            </div>
            <pre class="code mono">{{ content }}</pre>
          </template>

          <div v-if="tab !== 'files'" class="foot-actions">
            <a class="btn btn-sm btn-primary" :href="href" download><Download :size="13" />{{ t('export.download') }} ({{ t('export.files.count', { n: files.length }) }})</a>
          </div>
        </template>
      </section>
    </div>
  </Modal>
</template>

<style scoped>
.exp { display: grid; grid-template-columns: 270px minmax(0, 1fr); gap: 20px; min-height: 460px; }
.formats { display: flex; flex-direction: column; gap: 14px; }
.grp { display: flex; flex-direction: column; gap: 6px; }
.grp h4 { margin: 0 0 2px; font-size: 11px; letter-spacing: .06em; text-transform: uppercase; color: var(--faint); font-weight: 600; }
.formats button { display: flex; gap: 10px; align-items: flex-start; text-align: left; padding: 9px 11px; border: 1px solid var(--line-strong); border-radius: var(--radius); background: var(--surface); }
.formats button:hover { background: var(--surface-2); }
.formats button.on { border-color: var(--accent); background: var(--accent-soft); }
.formats .ico { margin-top: 2px; color: var(--muted); flex: none; }
.formats button.on .ico { color: var(--accent); }
.formats .txt { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
.formats b { font-size: 13.5px; font-weight: 600; }
.formats button.on b { color: var(--accent); }
.formats small { font-size: 11.5px; color: var(--muted); line-height: 1.35; }
.main { min-width: 0; display: flex; flex-direction: column; gap: 12px; }

.summary { display: grid; grid-template-columns: 1.6fr 1fr 1fr 1fr; gap: 10px; }
.stat { position: relative; padding: 10px 12px; border: 1px solid var(--line); border-radius: var(--radius); background: var(--surface-2); display: flex; flex-direction: column; gap: 2px; overflow: hidden; }
.stat b { font-size: 22px; font-weight: 700; letter-spacing: -.01em; line-height: 1.1; }
.stat .of { font-size: 14px; color: var(--faint); font-weight: 500; }
.stat small { font-size: 11.5px; color: var(--muted); }
.stat.warn { background: var(--risk-bg); border-color: var(--risk-line); }
.stat.warn b { color: var(--risk); }
.meter { height: 4px; background: var(--surface-sunken); border-radius: 2px; margin-top: 6px; display: block; overflow: hidden; }
.meter u { display: block; height: 100%; background: var(--ok-dot); border-radius: 2px; }
.honest { display: flex; gap: 6px; align-items: flex-start; margin: 0; font-size: 12.5px; line-height: 1.45; }
.honest svg { flex: none; margin-top: 2px; }

.tabsbar { display: flex; gap: 2px; border-bottom: 1px solid var(--line); }
.tabsbar button { padding: 8px 12px; border: 0; background: transparent; font-size: 13px; color: var(--muted); border-bottom: 2px solid transparent; margin-bottom: -1px; display: inline-flex; align-items: center; gap: 6px; }
.tabsbar button:hover { color: var(--text); }
.tabsbar button.on { color: var(--accent); border-bottom-color: var(--accent); font-weight: 600; }
.bdg { background: var(--surface-sunken); color: var(--text-2); border-radius: 10px; font-size: 11px; padding: 0 6px; line-height: 17px; font-weight: 600; }
.tabsbar button.on .bdg { background: var(--accent-soft); color: var(--accent); }

.pane { display: flex; flex-direction: column; gap: 10px; max-height: 50vh; overflow: auto; padding-right: 4px; }
.pane h5 { margin: 8px 0 0; font-size: 12px; letter-spacing: .04em; text-transform: uppercase; color: var(--muted); display: flex; align-items: center; gap: 6px; }
.rows { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 8px; }
.rows li { display: flex; gap: 10px; align-items: flex-start; padding: 10px 12px; border: 1px solid var(--line); border-radius: var(--radius); background: var(--surface); }
.row-g .okc { color: var(--ok); flex: none; margin-top: 1px; }
.row-n .wc { color: var(--risk); flex: none; margin-top: 1px; }
.row-n .ic { color: var(--accent); flex: none; margin-top: 1px; }
.row-n.warn { background: var(--risk-bg); border-color: var(--risk-line); }
.body { min-width: 0; flex: 1; display: flex; flex-direction: column; gap: 4px; }
.head { display: flex; gap: 6px; align-items: center; flex-wrap: wrap; font-size: 13.5px; }
.items { display: flex; flex-wrap: wrap; gap: 4px; }
.items code, code.key { font-family: var(--mono); font-size: 11.5px; background: var(--surface-sunken); border-radius: 4px; padding: 1px 6px; color: var(--text-2); overflow-wrap: anywhere; }
code.key { margin-right: 4px; }
.why { margin: 0; font-size: 12.5px; line-height: 1.5; color: var(--text-2); }
.small { font-size: 12px; margin: 0; }

.tabs2 { display: flex; gap: 4px; overflow-x: auto; }
.tabs2 button { border: 1px solid transparent; background: transparent; padding: 4px 10px; border-radius: 6px; font-family: var(--mono); font-size: 12px; white-space: nowrap; color: var(--muted); }
.tabs2 button.on { background: var(--surface-sunken); color: var(--text); border-color: var(--line); }
.code { margin: 0; padding: 14px 16px; background: var(--ink); color: var(--on-dark); border-radius: var(--radius); font-size: 12px; line-height: 1.55; overflow: auto; max-height: 50vh; white-space: pre; }
.foot-actions { display: flex; justify-content: flex-end; }
@media (max-width: 900px) { .exp { grid-template-columns: 1fr; } .summary { grid-template-columns: 1fr 1fr; } }
</style>
