<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { onBeforeRouteLeave, useRoute, useRouter } from 'vue-router'
import { t } from '@/i18n'
import BulkCard from '@/editor/BulkCard.vue'
import Canvas from '@/editor/Canvas.vue'
import AiOffHint from '@/editor/AiOffHint.vue'
import CommandPalette from '@/editor/CommandPalette.vue'
import EditorHeader from '@/editor/EditorHeader.vue'
import ExportModal from '@/editor/ExportModal.vue'
import FailureMatrix from '@/editor/FailureMatrix.vue'
import Inspector from '@/editor/Inspector.vue'
import Library from '@/editor/Library.vue'
import PlacementWizard from '@/editor/PlacementWizard.vue'
import PromptBar from '@/editor/PromptBar.vue'
import RightPanel from '@/editor/RightPanel.vue'
import ScoreModal from '@/editor/ScoreModal.vue'
import ShortcutsModal from '@/editor/ShortcutsModal.vue'
import SimWindow from '@/editor/SimWindow.vue'
import StoryCaption from '@/editor/StoryCaption.vue'
import VersionsModal from '@/editor/VersionsModal.vue'
import WhatIfBar from '@/editor/WhatIfBar.vue'
import { decodeScenario, scenarioFits, SIM_PARAM } from '@/lib/simLink'
import { useAi } from '@/stores/ai'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'
import { usePlayback } from '@/stores/playback'
import { useToasts } from '@/stores/toasts'
import { useUi } from '@/stores/ui'

const props = defineProps<{ id: string }>()
const app = useApp()
const editor = useEditor()
const ai = useAi()
const ui = useUi()
const playback = usePlayback()
const toasts = useToasts()
const route = useRoute()
const router = useRouter()
const library = ref<InstanceType<typeof Library> | null>(null)

async function open(id: string) {
  ai.reset()
  ui.modal = null
  await editor.load(id)
  // no AI on the server: there is no chat to read, and the request could only answer "nothing"
  if (ai.enabled) void ai.loadChat()
  if (!editor.loadError) openLinkedFailure()
}

/** A link made by "copy a link to this scenario" (`?sim=`) opens the diagram with that failure on screen; the parameter is spent, so a reload does not repeat it. */
function openLinkedFailure() {
  const raw = route.query[SIM_PARAM]
  if (raw === undefined) return
  void router.replace({ query: { ...route.query, [SIM_PARAM]: undefined } })
  const sc = decodeScenario(Array.isArray(raw) ? raw[0] : raw)
  if (sc && scenarioFits(sc, editor.arch)) void editor.runScenario(sc)
  else toasts.error(t('sim.link.stale'))
}

onMounted(() => {
  void open(props.id)
  window.addEventListener('keydown', onKey)
  window.addEventListener('beforeunload', onUnload)
})
watch(() => props.id, (id) => void open(id))

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKey)
  window.removeEventListener('beforeunload', onUnload)
  void editor.flushSave().finally(() => { editor.reset(); ai.reset() })
})
onBeforeRouteLeave(async () => { await editor.flushSave() })

function onUnload(e: BeforeUnloadEvent) {
  if (editor.saveState === 'dirty' || editor.saveState === 'saving' || editor.saveState === 'error') {
    e.preventDefault()
    e.returnValue = ''
  }
}

function typing(el: EventTarget | null): boolean {
  const n = el as HTMLElement | null
  return !!n && (n.tagName === 'INPUT' || n.tagName === 'TEXTAREA' || n.tagName === 'SELECT' || n.isContentEditable)
}

/** A control that handles the key itself (a button takes Space, a slider the arrows): the editor leaves it alone. */
function control(el: EventTarget | null): boolean {
  const n = el as HTMLElement | null
  return !!n && (n.tagName === 'BUTTON' || n.tagName === 'A' || ['slider', 'tab', 'separator', 'menuitem', 'option'].includes(n.getAttribute?.('role') ?? ''))
}

function onKey(e: KeyboardEvent) {
  const mod = e.ctrlKey || e.metaKey
  if (mod && e.key.toLowerCase() === 'k') { e.preventDefault(); ui.palette = !ui.palette; return }
  if (mod && e.key.toLowerCase() === 's') { e.preventDefault(); void editor.flushSave(); return }
  if (ui.palette || typing(e.target) || ui.modal) return
  if (mod && e.key.toLowerCase() === 'z') { e.preventDefault(); e.shiftKey ? editor.redo() : editor.undo(); return }
  if (mod && e.key.toLowerCase() === 'y') { e.preventDefault(); editor.redo(); return }
  if (mod && e.key.toLowerCase() === 'a') { e.preventDefault(); editor.selectNodes(editor.shown.nodes.map((n) => n.id)); return }
  if (mod && e.key.toLowerCase() === 'd') {
    e.preventDefault()
    if (editor.multi.length) editor.duplicateNodes(editor.multi)
    else if (editor.selection?.type === 'node') editor.duplicateNode(editor.selection.id)
    return
  }
  if (mod || e.altKey) return
  if (e.key === 'Delete' || e.key === 'Backspace') {
    if (editor.multi.length) { e.preventDefault(); editor.removeNodes(editor.multi) }
    else if (editor.selection) { e.preventDefault(); editor.removeSelection() }
    return
  }
  if (e.key === '?') { e.preventDefault(); ui.modal = 'shortcuts'; return }
  // the panels fold with the keys that look like their edges (by the position of the key, so that the Russian layout has them too)
  if (e.code === 'BracketLeft') { e.preventDefault(); ui.libraryOpen = !ui.libraryOpen; return }
  if (e.code === 'BracketRight') { e.preventDefault(); ui.rightOpen = !ui.rightOpen; return }
  // a failure on screen plays and steps from the keyboard
  if (editor.sim?.result && playback.available && !control(e.target)) {
    if (e.code === 'Space') { e.preventDefault(); playback.toggle(); return }
    if (e.key === 'ArrowRight') { e.preventDefault(); playback.step(1); return }
    if (e.key === 'ArrowLeft') { e.preventDefault(); playback.step(-1); return }
  }
  if (e.key === 'Escape') {
    if (editor.pickingNode || editor.connectFrom || editor.tool !== 'select') editor.setTool('select')
    else if (editor.highlight) editor.highlight = null
    else if (editor.sim) editor.exitSim()
    else if (editor.multi.length) editor.setMulti([])
    else editor.select(null)
    return
  }
  if (e.key === '/') { e.preventDefault(); library.value?.focus(); return }
  if (e.key === 'v') editor.setTool('select')
  else if (e.key === 'c' && app.canEdit) editor.setTool('connect')
  else if (e.key === 'z' && app.canEdit) editor.setTool('zone')
}

const conflict = computed(() => editor.saveState === 'conflict')
</script>

<template>
  <div class="editor">
    <EditorHeader />
    <AiOffHint />
    <PromptBar v-if="ui.chrome.promptBar" />
    <div class="body">
      <Library ref="library" />
      <main class="center">
        <div v-if="editor.loading" class="state skel-canvas" aria-busy="true" :aria-label="t('app.loading')" data-testid="editor-skeleton">
          <div class="skel sk-zone" style="left: 12%; top: 22%; width: 34%; height: 58%" />
          <div class="skel sk-zone" style="left: 52%; top: 22%; width: 34%; height: 58%" />
          <div v-for="n in 6" :key="n" class="skel sk-node" :style="{ left: 16 + (n % 2) * 40 + '%', top: 28 + Math.floor((n - 1) / 2) * 17 + '%' }" />
        </div>
        <div v-else-if="editor.loadError" class="state"><div class="banner banner-err">{{ editor.loadError }}</div><router-link class="btn" to="/">{{ t('editor.crumb.projects') }}</router-link></div>
        <template v-else>
          <Canvas />
          <Inspector />
          <BulkCard v-if="editor.multi.length > 1" />
          <StoryCaption />
          <SimWindow v-if="editor.sim && ui.simMode === 'float'" />
          <WhatIfBar />
          <div v-if="conflict" class="conflict banner banner-err" role="alert">
            <b>{{ t('editor.conflict.title') }}</b>
            <p>{{ t('editor.conflict.text') }}</p>
            <button class="btn btn-primary btn-sm" @click="editor.reloadFromServer()">{{ t('editor.conflict.reload') }}</button>
          </div>
          <div v-else-if="editor.saveState === 'error'" class="conflict banner banner-warn" role="alert">{{ t('editor.save.error') }}: {{ editor.saveError }}</div>
          <div v-if="editor.preview" class="previewbar">
            {{ editor.preview.kind === 'compare' ? t('compare.banner', { label: editor.preview.label ?? '' }) : t('proposal.previewing') }}
            <button v-if="editor.preview.kind === 'compare'" class="exit" @click="editor.setPreview(null)">{{ t('compare.exit') }}</button>
          </div>
        </template>
      </main>
      <RightPanel />
    </div>

    <VersionsModal v-if="ui.modal === 'versions'" />
    <ExportModal v-if="ui.modal === 'export'" />
    <FailureMatrix v-if="ui.modal === 'matrix'" />
    <ScoreModal v-if="ui.modal === 'score'" />
    <ShortcutsModal v-if="ui.modal === 'shortcuts'" />
    <PlacementWizard v-if="ui.modal === 'placement'" mode="replace" @close="ui.modal = null" />
    <CommandPalette v-if="ui.palette" />
  </div>
</template>

<style scoped>
.editor { height: 100%; display: flex; flex-direction: column; overflow: hidden; }
.body { flex: 1; display: flex; min-height: 0; }
.center { flex: 1; position: relative; min-width: 0; container-type: inline-size; }
.state { position: absolute; inset: 0; display: flex; flex-direction: column; gap: 12px; align-items: center; justify-content: center; }
.skel-canvas { display: block; background: var(--bg); }
.sk-zone { position: absolute; border-radius: var(--radius-lg); opacity: .6; }
.sk-node { position: absolute; width: 212px; height: 64px; max-width: 30%; border-radius: var(--radius); }
.conflict { position: absolute; top: 14px; left: 50%; transform: translateX(-50%); z-index: 80; max-width: 520px; box-shadow: var(--shadow-2); }
.conflict p { margin: 4px 0 8px; }
.previewbar { position: absolute; top: var(--bars-top, 56px); left: 50%; transform: translateX(-50%); z-index: 58; background: var(--accent); color: #fff; font-size: 12.5px; font-weight: 600; padding: 5px 14px; border-radius: 14px; box-shadow: var(--shadow-2); display: flex; align-items: center; gap: 10px; }
.previewbar .exit { border: 1px solid rgba(255, 255, 255, .6); background: transparent; color: #fff; border-radius: 10px; padding: 1px 10px; font-size: 12px; }
.previewbar .exit:hover { background: rgba(255, 255, 255, .15); }
</style>
