<script setup lang="ts">
import { computed } from 'vue'
import { useRouter } from 'vue-router'
import { History, Zap } from 'lucide-vue-next'
import BrandLogo from '@/components/BrandLogo.vue'
import LangSwitch from '@/components/LangSwitch.vue'
import * as api from '@/api/endpoints'
import { t } from '@/i18n'
import { errorMessage } from '@/lib/format'
import { scoreTone } from '@/lib/score'
import { useApp } from '@/stores/app'
import { useDialogs } from '@/stores/dialogs'
import { useEditor } from '@/stores/editor'
import { useToasts } from '@/stores/toasts'
import { useUi } from '@/stores/ui'

const app = useApp()
const editor = useEditor()
const ui = useUi()
const router = useRouter()
const dialogs = useDialogs()
const toasts = useToasts()

const score = computed(() => (editor.isEmpty ? null : editor.report?.score.value ?? null))
const tone = computed(() => scoreTone(score.value))
const confidence = computed(() => (editor.isEmpty || !editor.report ? null : Math.round(editor.report.score.coverage * 100)))
const lowConfidence = computed(() => confidence.value !== null && confidence.value < 70)
const saveText = computed(() => {
  switch (editor.saveState) {
    case 'saved': return t('editor.save.saved')
    case 'saving': return t('editor.save.saving')
    case 'dirty': return t('editor.save.dirty')
    case 'conflict': return t('editor.save.conflict')
    case 'error': return t('editor.save.error')
    default: return t('editor.save.readonly')
  }
})
const name = computed(() => editor.project?.name ?? '')

async function rename() {
  if (!app.canEdit || !editor.project) return
  const v = await dialogs.prompt({ title: t('projects.rename'), label: t('projects.create.name'), value: editor.project.name })
  if (!v?.trim() || v.trim() === editor.project.name) return
  try {
    await api.projects.patch(editor.projectId, { name: v.trim() })
    editor.project = { ...editor.project, name: v.trim() }
  } catch (e) {
    toasts.error(errorMessage(e))
  }
}

async function leave() {
  await editor.flushSave()
  router.push({ name: 'projects' })
}
</script>

<template>
  <header class="eh">
    <router-link to="/" class="brand" @click.prevent="leave"><BrandLogo /><span>{{ app.appName }}</span></router-link>
    <div class="crumbs">
      <a href="/" class="muted" @click.prevent="leave">{{ t('editor.crumb.projects') }}</a>
      <span class="sl">/</span>
      <button class="pname" :disabled="!app.canEdit" :title="t('projects.rename')" @click="rename">{{ name }}</button>
      <span class="meta">· v{{ editor.project?.headVersion ?? 0 }} · <span :class="'st-' + editor.saveState" :title="editor.saveError">{{ saveText }}</span></span>
    </div>
    <div class="mode" role="group">
      <button :class="{ on: ui.aiMode === 'ai' }" :title="app.boot?.ai.enabled ? '' : t('editor.mode.ai.off')" @click="ui.aiMode = 'ai'">{{ t('editor.mode.ai') }}</button>
      <button :class="{ on: ui.aiMode === 'manual' }" @click="ui.aiMode = 'manual'">{{ t('editor.mode.manual') }}</button>
    </div>
    <span class="spacer" />
    <button class="score" :class="[tone, lowConfidence && 'weak']" :title="lowConfidence ? t('checks.confidence.hint') : t('editor.score.title')" @click="ui.modal = 'score'">
      {{ t('editor.score') }}<b>{{ score === null ? '—' : Math.round(score) }}</b><span>/100</span><i v-if="lowConfidence" class="conf">{{ t('editor.score.confidence', { pct: confidence ?? 0 }) }}</i>
    </button>
    <button class="btn btn-dark" @click="ui.modal = 'matrix'"><Zap :size="14" />{{ t('editor.sim') }}</button>
    <button class="btn btn-dark" @click="ui.modal = 'versions'"><History :size="14" />{{ t('editor.versions') }}</button>
    <button class="btn btn-white" @click="ui.modal = 'export'"><span class="long">{{ t('editor.export') }}</span><span class="short">{{ t('editor.export.short') }}</span></button>
    <LangSwitch />
  </header>
</template>

<style scoped>
.eh { height: var(--header-h); background: var(--ink); color: var(--on-dark); display: flex; align-items: center; gap: 14px; padding: 0 20px; flex: none; }
.brand { display: flex; align-items: center; gap: 12px; color: #fff; font-weight: 650; font-size: 17px; letter-spacing: -0.01em; white-space: nowrap; }
.brand:hover { text-decoration: none; }
.crumbs { display: flex; align-items: baseline; gap: 6px; font-size: 14px; color: var(--on-dark-muted); min-width: 0; white-space: nowrap; }
.crumbs a { color: var(--on-dark-muted); }
.sl { color: #596176; }
.pname { background: transparent; border: 0; color: #fff; font-weight: 650; font-size: 14px; padding: 0 2px; max-width: 220px; overflow: hidden; text-overflow: ellipsis; border-radius: 4px; }
.pname:hover:not(:disabled) { background: var(--ink-3); }
.pname:disabled { cursor: default; }
.meta { color: var(--on-dark-muted); }
.st-saved { color: var(--on-dark-muted); } .st-dirty, .st-saving { color: #c4cbe0; } .st-conflict, .st-error { color: #ff8a8f; font-weight: 600; } .st-readonly { color: var(--on-dark-muted); }
.mode { display: inline-flex; padding: 3px; background: var(--ink-2); border: 1px solid #262c3d; border-radius: 9px; }
.mode button { border: 0; background: transparent; color: var(--on-dark-muted); height: 28px; padding: 0 14px; border-radius: 7px; font-size: 13.5px; font-weight: 500; white-space: nowrap; }
.mode button.on { background: var(--accent); color: #fff; }
.mode button:not(.on):hover { color: #fff; }
.score { display: inline-flex; align-items: baseline; gap: 8px; height: 38px; padding: 0 16px; align-self: center; border-radius: var(--radius); border: 1px solid; background: transparent; font-size: 13.5px; white-space: nowrap; align-items: center; }
.score b { font-size: 19px; letter-spacing: -0.02em; }
.score span { opacity: .75; font-size: 12.5px; margin-left: -4px; }
.score.ok { color: #6fe39a; border-color: #1f6b3d; background: rgba(39, 194, 94, .1); }
.score.risk { color: #ffc266; border-color: #7a5410; background: rgba(243, 160, 12, .1); }
.score.crit { color: #ff8a8f; border-color: #8a2a30; background: rgba(192, 38, 45, .12); }
.score.weak { border-style: dashed; }
.score .conf { font-style: normal; font-size: 11.5px; opacity: .85; margin-left: 2px; }
.score.none { color: var(--on-dark-muted); border-color: #3a4256; }
.long { display: inline; } .short { display: none; }
@media (max-width: 1600px) { .long { display: none; } .short { display: inline; } .crumbs .meta { display: none; } }
@media (max-width: 1360px) { .btn-dark { font-size: 0; gap: 0; padding: 0 10px; } .mode button { padding: 0 10px; } }
</style>
