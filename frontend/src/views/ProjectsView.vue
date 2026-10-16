<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { Plus, Search, Pencil, Trash2, Boxes, Copy, Network } from 'lucide-vue-next'
import * as api from '@/api/endpoints'
import type { ProjectListItem } from '@/api/types'
import DiagramThumb from '@/components/DiagramThumb.vue'
import TemplateGallery from '@/components/TemplateGallery.vue'
import Modal from '@/components/Modal.vue'
import PlacementWizard from '@/editor/PlacementWizard.vue'
import TopBar from '@/components/TopBar.vue'
import { lang, t } from '@/i18n'
import { fmtRelative } from '@/lib/format'
import { errorMessage } from '@/lib/format'
import { scoreTone } from '@/lib/score'
import { useApp } from '@/stores/app'
import { useDialogs } from '@/stores/dialogs'
import { useToasts } from '@/stores/toasts'

const app = useApp()
const router = useRouter()
const toasts = useToasts()
const dialogs = useDialogs()

const items = ref<ProjectListItem[]>([])
const loading = ref(true)
const error = ref('')
const query = ref('')
const creating = ref(false)
const wizard = ref(false)
const form = ref({ name: '', templateId: '' })
const busy = ref(false)

type Sort = 'updated' | 'name' | 'worst' | 'best'
const SORTS: Sort[] = ['updated', 'name', 'worst', 'best']
const readSort = (): Sort => { try { const v = localStorage.getItem('ia_projects_sort') as Sort | null; return v && SORTS.includes(v) ? v : 'updated' } catch { return 'updated' } }
const sort = ref<Sort>(readSort())
watch(sort, (v) => { try { localStorage.setItem('ia_projects_sort', v) } catch { /* ignore */ } })
/** Show only projects with open critical findings or risks. */
const attention = ref(false)
const needsAttention = (p: ProjectListItem) => !!p.summary && p.summary.counts.critical + p.summary.counts.risk > 0

const filtered = computed(() => {
  const q = query.value.trim().toLowerCase()
  let list = items.value.filter((p) => (!q || (p.name + ' ' + p.description).toLowerCase().includes(q)) && (!attention.value || needsAttention(p)))
  // projects without a score sort last in both score orders
  const score = (p: ProjectListItem) => p.summary?.score ?? null
  const byScore = (dir: 1 | -1) => (a: ProjectListItem, b: ProjectListItem) => (score(a) === null ? 1 : score(b) === null ? -1 : (score(a)! - score(b)!) * dir)
  if (sort.value === 'name') list = [...list].sort((a, b) => a.name.localeCompare(b.name, lang.value, { numeric: true }))
  else if (sort.value === 'worst') list = [...list].sort(byScore(1))
  else if (sort.value === 'best') list = [...list].sort(byScore(-1))
  else list = [...list].sort((a, b) => Date.parse(b.updatedAt) - Date.parse(a.updatedAt))
  return list
})
const attentionCount = computed(() => items.value.filter(needsAttention).length)
const templates = computed(() => [...(app.boot?.templates ?? [])].sort((a, b) => a.order - b.order))

async function load() {
  loading.value = true
  try {
    items.value = (await api.projects.list()).projects
    error.value = ''
  } catch (e) {
    error.value = errorMessage(e)
  } finally {
    loading.value = false
  }
}

function openCreate() {
  form.value = { name: '', templateId: '' }
  creating.value = true
}

function openWizard() {
  creating.value = false
  wizard.value = true
}

async function create() {
  if (!form.value.name.trim()) return
  busy.value = true
  try {
    const r = await api.projects.create({ name: form.value.name.trim(), templateId: form.value.templateId || undefined, lang: lang.value })
    router.push({ name: 'editor', params: { id: r.id } })
  } catch (e) {
    toasts.error(errorMessage(e))
  } finally {
    busy.value = false
  }
}

async function rename(p: ProjectListItem) {
  const name = await dialogs.prompt({ title: t('projects.rename'), label: t('projects.create.name'), value: p.name })
  if (!name?.trim() || name.trim() === p.name) return
  try {
    await api.projects.patch(p.id, { name: name.trim() })
    p.name = name.trim()
  } catch (e) {
    toasts.error(errorMessage(e))
  }
}

/** A copy of the working diagram under a new name (versions and accepted risks stay with the original). */
async function duplicate(p: ProjectListItem) {
  const name = await dialogs.prompt({ title: t('projects.duplicate'), label: t('projects.duplicate.prompt'), value: t('projects.duplicate.name', { name: p.name }) })
  if (!name?.trim()) return
  try {
    const src = await api.projects.get(p.id)
    await api.projects.create({ name: name.trim(), description: p.description, architecture: src.architecture, lang: lang.value })
    toasts.ok(t('projects.duplicate.done', { name: name.trim() }))
    await load()
  } catch (e) {
    toasts.error(errorMessage(e))
  }
}

async function remove(p: ProjectListItem) {
  if (!(await dialogs.confirm({ title: t('projects.delete.confirm', { name: p.name }), danger: true, confirmLabel: t('common.delete') }))) return
  try {
    await api.projects.remove(p.id)
    items.value = items.value.filter((x) => x.id !== p.id)
  } catch (e) {
    toasts.error(errorMessage(e))
  }
}

onMounted(load)
</script>

<template>
  <div class="page">
    <TopBar />
    <main class="content">
      <div class="head">
        <h1>{{ t('projects.title') }}</h1>
        <div class="search">
          <Search :size="15" />
          <input v-model="query" class="input" :placeholder="t('common.search')" />
        </div>
        <label v-if="items.length > 1" class="sort"><span class="sr-only">{{ t('projects.sort') }}</span>
          <select v-model="sort" class="select" :aria-label="t('projects.sort')">
            <option v-for="x in SORTS" :key="x" :value="x">{{ t('projects.sort.' + x) }}</option>
          </select>
        </label>
        <button v-if="app.canEdit" class="btn btn-primary" data-tour="new-project" @click="openCreate"><Plus :size="16" />{{ t('projects.new') }}</button>
      </div>

      <div v-if="error" class="banner banner-err">{{ error }} <button class="btn btn-sm" style="margin-left: 8px" @click="load">{{ t('common.retry') }}</button></div>
      <div v-else-if="loading" class="grid" aria-busy="true" :aria-label="t('app.loading')" data-testid="projects-skeleton">
        <div v-for="n in 6" :key="n" class="card sk-card">
          <div class="skel sk-thumb" />
          <div class="sk-body"><div class="skel" style="width: 58%; height: 16px" /><div class="skel" style="width: 92%" /><div class="skel" style="width: 40%; height: 26px" /></div>
        </div>
      </div>
      <div v-else-if="!items.length" class="empty card card-pad" data-tour="project-list">
        <Boxes :size="32" style="color: var(--faint)" />
        <p>{{ t('projects.empty') }}</p>
        <button v-if="app.canEdit" class="btn btn-primary" @click="openCreate"><Plus :size="16" />{{ t('projects.new') }}</button>
      </div>

      <div v-if="items.length && attentionCount" class="chips">
        <button class="chip" :class="{ on: attention }" :aria-pressed="attention" @click="attention = !attention">{{ t('projects.attention', { n: attentionCount }) }}</button>
      </div>

      <div v-if="items.length" class="grid" data-tour="project-list">
        <article v-for="(p, i) in filtered" :key="p.id" class="card project rise" :style="{ '--i': Math.min(i, 10) }" @click="router.push({ name: 'editor', params: { id: p.id } })">
          <DiagramThumb :id="p.id" :thumb="p.thumb" :node-count="p.nodeCount" />
          <div class="pbody">
          <div class="top">
            <h2 class="grow truncate">{{ p.name }}</h2>
            <template v-if="app.canEdit">
              <button class="btn btn-ghost btn-icon" :title="t('projects.rename')" :aria-label="t('projects.rename')" @click.stop="rename(p)"><Pencil :size="15" /></button>
              <button class="btn btn-ghost btn-icon" :title="t('projects.duplicate')" :aria-label="t('projects.duplicate')" @click.stop="duplicate(p)"><Copy :size="15" /></button>
              <button class="btn btn-ghost btn-icon" :title="t('common.delete')" :aria-label="t('common.delete')" @click.stop="remove(p)"><Trash2 :size="15" /></button>
            </template>
          </div>
          <p class="desc muted">{{ p.description || ' ' }}</p>
          <div class="meta">
            <div class="score" :class="scoreTone(p.summary?.score)">
              <b>{{ p.summary ? Math.round(p.summary.score) : '—' }}</b>
              <span>{{ p.summary ? '/100' : t('projects.noscore') }}</span>
            </div>
            <div v-if="p.summary" class="counts">
              <span v-if="p.summary.counts.critical" class="tag tag-crit">{{ p.summary.counts.critical }} {{ t('checks.critical') }}</span>
              <span v-if="p.summary.counts.risk" class="tag tag-risk">{{ p.summary.counts.risk }} {{ t('checks.risk') }}</span>
              <span v-if="p.summary.counts.noData" class="tag tag-nodata">{{ p.summary.counts.noData }} {{ t('checks.nodata') }}</span>
              <span v-if="!p.summary.counts.critical && !p.summary.counts.risk && !p.summary.counts.noData" class="tag tag-ok">{{ t('checks.none') }}</span>
              <span v-if="p.summary.coverage < 0.7" class="tag tag-nodata" :title="t('checks.confidence.hint')">{{ t('editor.score.confidence', { pct: Math.round(p.summary.coverage * 100) }) }}</span>
            </div>
          </div>
          <div class="foot faint">
            <span>{{ t('projects.nodes', { n: p.nodeCount }) }}</span>
            <span>·</span>
            <span>{{ t('projects.updated', { when: fmtRelative(p.updatedAt) }) }}</span>
            <span v-if="p.owner" class="truncate">· {{ p.owner }}</span>
          </div>
          </div>
        </article>
        <div v-if="!filtered.length" class="empty" style="grid-column: 1 / -1">{{ t('library.nothing') }}</div>
      </div>
    </main>

    <PlacementWizard v-if="wizard" mode="new" @close="wizard = false" />
    <Modal v-if="creating" :title="t('projects.create.title')" size="lg" @close="creating = false">
      <form class="col" style="gap: 16px" @submit.prevent="create">
        <div class="field">
          <label>{{ t('projects.create.name') }}</label>
          <input v-model="form.name" class="input" required autofocus maxlength="120" />
        </div>
        <button type="button" class="wizard-card" @click="openWizard">
          <span class="wi"><Network :size="20" /></span>
          <span class="wt"><b>{{ t('projects.create.placement') }}</b><span class="muted">{{ t('projects.create.placement.hint') }}</span></span>
          <span class="tag tag-accent tag-mono">1 · 2 · 3 · 5 · 10</span>
        </button>
        <div class="field">
          <label id="tpl-from">{{ t('projects.create.from') }}</label>
          <TemplateGallery v-model="form.templateId" :templates="templates" />
        </div>
        <button type="submit" class="sr-only" tabindex="-1">ok</button>
      </form>
      <template #footer>
        <button class="btn" @click="creating = false">{{ t('common.cancel') }}</button>
        <button class="btn btn-primary" :disabled="busy || !form.name.trim()" @click="create">{{ t('common.create') }}</button>
      </template>
    </Modal>
  </div>
</template>

<style scoped>
.page { height: 100%; display: flex; flex-direction: column; }
.content { flex: 1; overflow: auto; padding: 28px 32px 48px; max-width: 1280px; width: 100%; margin: 0 auto; }
.head { display: flex; align-items: center; gap: 12px; margin-bottom: 22px; flex-wrap: wrap; }
.head h1 { font-size: 24px; flex: 1; }
.search { position: relative; width: 260px; }
.search svg { position: absolute; left: 10px; top: 10px; color: var(--faint); }
.search .input { padding-left: 32px; }
.grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(320px, 1fr)); gap: 16px; }
.project { padding: 0; overflow: hidden; cursor: pointer; display: flex; flex-direction: column; transition: box-shadow var(--dur-fast), border-color var(--dur-fast); content-visibility: auto; contain-intrinsic-size: auto 330px; }
.pbody { padding: 14px 18px 16px; display: flex; flex-direction: column; gap: 8px; flex: 1; }
.sort { width: 190px; }
.chips { display: flex; gap: 8px; margin: -8px 0 16px; }
.chip.on { background: var(--accent-soft); border-color: var(--accent-line); color: var(--accent); font-weight: 600; }
.project:hover { box-shadow: var(--shadow-2); border-color: var(--line-strong); }
.top { display: flex; align-items: center; gap: 4px; }
.top h2 { font-size: 16px; }
.desc { margin: 0; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; font-size: 13px; }
.meta { display: flex; align-items: center; gap: 12px; margin-top: 4px; }
.score { display: flex; align-items: baseline; gap: 3px; padding: 4px 10px; border-radius: var(--radius-sm); border: 1px solid var(--line); background: var(--surface-2); }
.score b { font-size: 20px; letter-spacing: -0.02em; }
.score span { font-size: 11.5px; color: var(--muted); }
.score.ok { background: var(--ok-bg); border-color: var(--ok-line); color: var(--ok); }
.score.risk { background: var(--risk-bg); border-color: var(--risk-line); color: var(--risk); }
.score.crit { background: var(--crit-bg); border-color: var(--crit-line); color: var(--crit); }
.counts { display: flex; gap: 6px; flex-wrap: wrap; }
.foot { display: flex; gap: 6px; font-size: 12px; margin-top: auto; padding-top: 4px; }
.empty { display: flex; flex-direction: column; align-items: center; gap: 12px; }
.sk-card { overflow: hidden; display: flex; flex-direction: column; }
.sk-thumb { height: 128px; border-radius: 0; }
.sk-body { padding: 16px 18px 18px; display: flex; flex-direction: column; gap: 12px; }
.wizard-card { display: flex; align-items: center; gap: 12px; text-align: left; padding: 12px 14px; border: 1px solid var(--accent-line); border-radius: var(--radius); background: var(--accent-soft); transition: box-shadow var(--dur-fast), transform var(--dur-fast); }
.wizard-card:hover { box-shadow: var(--shadow-2); transform: translateY(-1px); }
.wizard-card .wi { width: 38px; height: 38px; border-radius: 10px; background: var(--accent); color: #fff; display: grid; place-items: center; flex: none; }
.wizard-card .wt { display: flex; flex-direction: column; gap: 2px; flex: 1; font-size: 13px; }
@media (max-width: 640px) { .content { padding: 18px 16px; } }
</style>
