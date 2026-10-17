<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { onBeforeRouteLeave, onBeforeRouteUpdate, useRoute, useRouter } from 'vue-router'
import { t } from '@/i18n'
import BulkCard from '@/editor/BulkCard.vue'
import { focusDiagram } from '@/editor/canvasFocus'
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
import SaveStatusBar from '@/editor/SaveStatusBar.vue'
import ScoreModal from '@/editor/ScoreModal.vue'
import ShortcutsModal from '@/editor/ShortcutsModal.vue'
import SimWindow from '@/editor/SimWindow.vue'
import StoryCaption from '@/editor/StoryCaption.vue'
import VersionsModal from '@/editor/VersionsModal.vue'
import WhatIfBar from '@/editor/WhatIfBar.vue'
import LiveRegion from '@/components/LiveRegion.vue'
import { announce, clearAnnouncement, describeChange } from '@/lib/announce'
import { stepFinding } from '@/lib/navigate'
import { pageHeading } from '@/lib/pageFocus'
import { blocked, matchShortcut, repeats, type WindowShortcutId } from '@/lib/shortcuts'
import { decodeScenario, scenarioFits, SIM_PARAM } from '@/lib/simLink'
import { useAi } from '@/stores/ai'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'
import { usePlayback } from '@/stores/playback'
import { useToasts } from '@/stores/toasts'
import { useUi } from '@/stores/ui'
import { useUnsaved } from '@/stores/unsaved'

const props = defineProps<{ id: string }>()
const app = useApp()
const editor = useEditor()
const ai = useAi()
const ui = useUi()
const playback = usePlayback()
const toasts = useToasts()
const unsaved = useUnsaved()
const route = useRoute()
const router = useRouter()
const library = ref<InstanceType<typeof Library> | null>(null)
const skips = ref<HTMLElement | null>(null)

async function open(id: string) {
  unsaved.clear() // the offer of the project that was open belongs to it: it is not shown beside this one's load, or its error
  ai.reset()
  ui.modal = null
  await editor.load(id)
  // no AI on the server: there is no chat to read, and the request could only answer "nothing"
  if (ai.enabled) void ai.loadChat()
  if (!editor.loadError) {
    void unsaved.check(id) // edits of an earlier visit that never reached the server are offered back
    openLinkedFailure()
  }
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
  window.addEventListener('online', onOnline)
})
watch(() => props.id, (id) => void open(id))

// What an edit did to the verdict is said to a screen reader (lib/announce.ts): the page shows it by colour and by numbers, which says nothing to somebody who does
// not look. Only the edits of the user (the store sets `reportChange` for those and for nothing else), and only when something changed.
watch(() => editor.reportChange, (c) => {
  const text = c ? describeChange(c.prev, c.next, app.rules) : null
  if (text) announce(text)
})

onBeforeUnmount(() => {
  clearAnnouncement()
  window.removeEventListener('keydown', onKey)
  window.removeEventListener('beforeunload', onUnload)
  window.removeEventListener('online', onOnline)
  unsaved.clear()
  void editor.flushSave().finally(() => { editor.reset(); ai.reset() })
})
// Leaving the project (the list, the admin panel, another project) stores what is edited; what cannot be stored is kept in this browser and the person is told
onBeforeRouteLeave(async () => { await unsaved.settle() })
onBeforeRouteUpdate(async () => { await unsaved.settle() })

/** The network is back: a save that was waiting for its pause goes out now. */
function onOnline() {
  editor.retryNow()
}

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

/** The project is there to export, to run a matrix on, to look through the findings of. */
const ready = () => !editor.loading && !editor.loadError && !!editor.projectId

/** What each shortcut of the table does here (`lib/shortcuts.ts`): an action that has nothing to do right now answers `false`, and the key goes on to the browser. */
const actions: Record<WindowShortcutId, (e: KeyboardEvent) => boolean | void> = {
  palette: () => { ui.palette = !ui.palette },
  shortcuts: () => { ui.modal = 'shortcuts' },
  save: () => { void editor.flushSave() },
  escape: () => {
    if (editor.pickingNode || editor.connectFrom || editor.tool !== 'select') editor.setTool('select')
    else if (editor.highlight) editor.highlight = null
    else if (editor.sim) editor.exitSim()
    else if (editor.multi.length) editor.setMulti([])
    else editor.select(null)
  },
  library: () => library.value?.focus(),
  // the panels fold with the keys that look like their edges
  foldLibrary: () => { ui.libraryOpen = !ui.libraryOpen },
  foldRight: () => { ui.rightOpen = !ui.rightOpen },
  export: () => { if (!ready()) return false; ui.modal = 'export' },
  matrix: () => { if (!ready()) return false; ui.modal = 'matrix' },
  findingNext: () => ready() && stepFinding(1),
  findingPrev: () => ready() && stepFinding(-1),
  undo: () => editor.undo(),
  redo: () => editor.redo(),
  duplicate: () => {
    if (editor.multi.length) editor.duplicateNodes(editor.multi)
    else if (editor.selection?.type === 'node') editor.duplicateNode(editor.selection.id)
  },
  delete: () => {
    if (editor.multi.length) editor.removeNodes(editor.multi)
    else if (editor.selection) editor.removeSelection()
    else return false
  },
  selectAll: () => editor.selectNodes(editor.shown.nodes.map((n) => n.id)),
  layout: () => { if (!app.canEdit || editor.readonly || editor.isEmpty) return false; editor.autoLayout() },
  'tool.select': () => editor.setTool('select'),
  'tool.connect': () => { if (!app.canEdit) return false; editor.setTool('connect') },
  'tool.zone': () => { if (!app.canEdit) return false; editor.setTool('zone') },
  zoomIn: () => ui.requestZoom(1),
  zoomOut: () => ui.requestZoom(-1),
  fit: () => { editor.fitToken++ },
  focus: () => editor.setFocusOn(!editor.focusOn),
  // a failure on screen plays and steps from the keyboard
  play: () => { if (!editor.sim?.result || !playback.available) return false; playback.toggle() },
  step: (e) => { if (!editor.sim?.result || !playback.available) return false; playback.step(e.key === 'ArrowRight' ? 1 : -1) },
}

/**
 * The skip links are the first stops of the page, and the page's heading comes after them: a project opened from the list puts the focus on that heading (lib/pageFocus.ts),
 * and the Tab key goes on from there, with the links behind it. From the heading itself, Tab goes to the first of them (Shift+Tab, and Tab from anywhere else, are the
 * browser's), so that the way to the first card is the same two key presses as on a page that was loaded afresh.
 */
function enterSkips(e: KeyboardEvent) {
  if (e.shiftKey || e.ctrlKey || e.altKey || e.metaKey || e.defaultPrevented) return
  const heading = pageHeading()
  const first = skips.value?.querySelector<HTMLElement>('a')
  if (!heading || !first || document.activeElement !== heading) return
  e.preventDefault()
  first.focus()
}

function onKey(e: KeyboardEvent) {
  if (e.isComposing) return
  if (e.key === 'Tab') return enterSkips(e)
  const id = matchShortcut(e)
  if (!id || blocked(id, { typing: typing(e.target), control: control(e.target), dialog: ui.palette || !!ui.modal })) return
  // a key held down does the thing once, unless repeating is what it is for (zoom, undo)
  if (e.repeat && !repeats(id)) { e.preventDefault(); return }
  if (actions[id](e) !== false) e.preventDefault()
}

/**
 * The skip links, the first stops of the page: the diagram, the checks, the library. Without them the keyboard walks the header, the library and the whole right
 * panel's toolbar to get anywhere. The link is a real link to the element (it works without the script), the click does better: the diagram takes the focus on
 * the card the Tab key stops at, the checks on their selected tab (or on the button that unfolds the panel), the library on its search.
 */
function skipTo(where: 'canvas' | 'checks' | 'library') {
  if (where === 'canvas') focusDiagram()
  else if (where === 'library') library.value?.focus()
  else {
    const panel = document.querySelector<HTMLElement>('[data-testid="right-panel"]')
    ;(panel?.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]') ?? panel?.querySelector<HTMLElement>('[data-testid="right-unfold"]'))?.focus()
  }
}
</script>

<template>
  <div class="editor">
    <nav ref="skips" class="skips" :aria-label="t('skip.label')">
      <a class="skip-link" href="#editor-canvas" @click.prevent="skipTo('canvas')">{{ t('skip.canvas') }}</a>
      <a class="skip-link" href="#editor-checks" @click.prevent="skipTo('checks')">{{ t('skip.checks') }}</a>
      <a class="skip-link" href="#editor-library" @click.prevent="skipTo('library')">{{ t('skip.library') }}</a>
    </nav>
    <EditorHeader />
    <AiOffHint />
    <PromptBar v-if="ui.chrome.promptBar" />
    <SaveStatusBar />
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
    <LiveRegion />
  </div>
</template>

<style scoped>
.editor { height: 100%; display: flex; flex-direction: column; overflow: hidden; }
/* the skip links are there for the keyboard: out of the way until one has the focus, then over the header */
.skips { position: fixed; left: 12px; top: 8px; z-index: 600; display: flex; gap: 6px; }
.skip-link:not(:focus) { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
.skip-link { padding: 7px 14px; border-radius: var(--radius); background: var(--accent); color: #fff; font-size: 13px; font-weight: 600; text-decoration: none; box-shadow: var(--shadow-2); }
.body { flex: 1; display: flex; min-height: 0; }
.center { flex: 1; position: relative; min-width: 0; container-type: inline-size; }
.state { position: absolute; inset: 0; display: flex; flex-direction: column; gap: 12px; align-items: center; justify-content: center; }
.skel-canvas { display: block; background: var(--bg); }
.sk-zone { position: absolute; border-radius: var(--radius-lg); opacity: .6; }
.sk-node { position: absolute; width: 212px; height: 64px; max-width: 30%; border-radius: var(--radius); }
.previewbar { position: absolute; top: var(--bars-top, 56px); left: 50%; transform: translateX(-50%); z-index: 58; background: var(--accent); color: #fff; font-size: 12.5px; font-weight: 600; padding: 5px 14px; border-radius: 14px; box-shadow: var(--shadow-2); display: flex; align-items: center; gap: 10px; }
.previewbar .exit { border: 1px solid rgba(255, 255, 255, .6); background: transparent; color: #fff; border-radius: 10px; padding: 1px 10px; font-size: 12px; }
.previewbar .exit:hover { background: rgba(255, 255, 255, .15); }
</style>
