<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { Plus, Search, Pencil, Trash2, Boxes } from 'lucide-vue-next'
import * as api from '@/api/endpoints'
import type { ProjectListItem } from '@/api/types'
import Modal from '@/components/Modal.vue'
import TopBar from '@/components/TopBar.vue'
import { lang, lt, t } from '@/i18n'
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
const form = ref({ name: '', templateId: '' })
const busy = ref(false)

const filtered = computed(() => {
  const q = query.value.trim().toLowerCase()
  return q ? items.value.filter((p) => (p.name + ' ' + p.description).toLowerCase().includes(q)) : items.value
})
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
        <button v-if="app.canEdit" class="btn btn-primary" @click="openCreate"><Plus :size="16" />{{ t('projects.new') }}</button>
      </div>

      <div v-if="error" class="banner banner-err">{{ error }} <button class="btn btn-sm" style="margin-left: 8px" @click="load">{{ t('common.retry') }}</button></div>
      <div v-else-if="loading" class="empty"><span class="spinner" /></div>
      <div v-else-if="!items.length" class="empty card card-pad">
        <Boxes :size="32" style="color: var(--faint)" />
        <p>{{ t('projects.empty') }}</p>
        <button v-if="app.canEdit" class="btn btn-primary" @click="openCreate"><Plus :size="16" />{{ t('projects.new') }}</button>
      </div>

      <div v-else class="grid">
        <article v-for="p in filtered" :key="p.id" class="card project" @click="router.push({ name: 'editor', params: { id: p.id } })">
          <div class="top">
            <h3 class="grow truncate">{{ p.name }}</h3>
            <template v-if="app.canEdit">
              <button class="btn btn-ghost btn-icon" :title="t('projects.rename')" @click.stop="rename(p)"><Pencil :size="15" /></button>
              <button class="btn btn-ghost btn-icon" :title="t('common.delete')" @click.stop="remove(p)"><Trash2 :size="15" /></button>
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
            </div>
          </div>
          <div class="foot faint">
            <span>{{ t('projects.nodes', { n: p.nodeCount }) }}</span>
            <span>·</span>
            <span>{{ t('projects.updated', { when: fmtRelative(p.updatedAt) }) }}</span>
            <span v-if="p.owner" class="truncate">· {{ p.owner }}</span>
          </div>
        </article>
        <div v-if="!filtered.length" class="empty" style="grid-column: 1 / -1">{{ t('library.nothing') }}</div>
      </div>
    </main>

    <Modal v-if="creating" :title="t('projects.create.title')" @close="creating = false">
      <form class="col" style="gap: 16px" @submit.prevent="create">
        <div class="field">
          <label>{{ t('projects.create.name') }}</label>
          <input v-model="form.name" class="input" required autofocus maxlength="120" />
        </div>
        <div class="field">
          <label>{{ t('projects.create.from') }}</label>
          <div class="templates">
            <label class="tpl" :class="{ on: !form.templateId }">
              <input v-model="form.templateId" type="radio" value="" />
              <b>{{ t('projects.create.blank') }}</b>
              <span class="muted">{{ t('projects.create.blank.hint') }}</span>
            </label>
            <label v-for="tpl in templates" :key="tpl.id" class="tpl" :class="{ on: form.templateId === tpl.id }">
              <input v-model="form.templateId" type="radio" :value="tpl.id" />
              <span class="row"><b class="grow">{{ lt(tpl.name) }}</b><span v-if="tpl.badge" class="tag tag-accent tag-mono">{{ tpl.badge }}</span></span>
              <span class="muted">{{ lt(tpl.description) }}</span>
            </label>
          </div>
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
.head { display: flex; align-items: center; gap: 12px; margin-bottom: 22px; }
.head h1 { font-size: 24px; flex: 1; }
.search { position: relative; width: 260px; }
.search svg { position: absolute; left: 10px; top: 10px; color: var(--faint); }
.search .input { padding-left: 32px; }
.grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(320px, 1fr)); gap: 16px; }
.project { padding: 16px 18px; cursor: pointer; display: flex; flex-direction: column; gap: 8px; transition: box-shadow .12s, border-color .12s; }
.project:hover { box-shadow: var(--shadow-2); border-color: var(--line-strong); }
.top { display: flex; align-items: center; gap: 4px; }
.top h3 { font-size: 16px; }
.desc { margin: 0; min-height: 2.9em; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; font-size: 13px; }
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
.templates { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
.tpl { display: flex; flex-direction: column; gap: 4px; border: 1px solid var(--line-strong); border-radius: var(--radius); padding: 10px 12px; cursor: pointer; font-size: 13px; }
.tpl:hover { border-color: #b8c2d4; }
.tpl.on { border-color: var(--accent); background: var(--accent-soft); }
.tpl input { position: absolute; opacity: 0; pointer-events: none; }
@media (max-width: 640px) { .templates { grid-template-columns: 1fr; } .content { padding: 18px 16px; } }
</style>
