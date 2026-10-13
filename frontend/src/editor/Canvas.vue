<script setup lang="ts">
import { computed, markRaw, nextTick, onMounted, ref, watch } from 'vue'
import { ConnectionMode, VueFlow, useVueFlow, type GraphNode, type NodeDragEvent } from '@vue-flow/core'
import { Background } from '@vue-flow/background'
import { MiniMap } from '@vue-flow/minimap'
import { LayoutDashboard, Minus, MousePointer2, PanelLeftOpen, Plus, Redo2, Spline, Square, Undo2 } from 'lucide-vue-next'
import '@vue-flow/core/dist/style.css'
import '@vue-flow/minimap/dist/style.css'
import { lt, t } from '@/i18n'
import { useApp } from '@/stores/app'
import { useEditor, type Layer } from '@/stores/editor'
import { useUi } from '@/stores/ui'
import ArchEdge from './ArchEdge.vue'
import ArchNode from './ArchNode.vue'
import ArchZone from './ArchZone.vue'
import { createFlowBuilder, nodeFlowId, zoneFlowId } from './flowModel'
import { lodOf } from '@/lib/lod'
import { MIN_ZONE_H, MIN_ZONE_W } from './layout'

const app = useApp()
const editor = useEditor()
const ui = useUi()
const nodeTypes = { card: markRaw(ArchNode), zone: markRaw(ArchZone) } as any
const edgeTypes = { arch: markRaw(ArchEdge) } as any

const { screenToFlowCoordinate, fitView, zoomIn, zoomOut, viewport, panBy, onNodesInitialized, addSelectedNodes, removeSelectedElements, findNode, getSelectedNodes } = useVueFlow()
const wrap = ref<HTMLElement | null>(null)

const build = createFlowBuilder()
const flow = computed(() => build(editor.shown, !editor.readonly))

// ------------------------------------------------------------------------------------- viewport
function updateCenter() {
  const r = wrap.value?.getBoundingClientRect()
  if (!r) return
  editor.viewCenter = screenToFlowCoordinate({ x: r.left + r.width / 2, y: r.top + r.height / 2 })
}

async function fit() {
  await nextTick()
  setTimeout(async () => {
    await fitView({ padding: 0.1, maxZoom: 1, duration: 0 })
    await panBy({ x: 0, y: 26 }) // leave room for the floating toolbar
    updateCenter()
  }, 80)
}

onNodesInitialized(() => { void fit() })
onMounted(() => { void fit() })
watch(() => editor.fitToken, () => void fit())

watch(() => editor.highlight, (h) => {
  if (!h) return
  const ids = new Set<string>()
  for (const n of h.nodes) ids.add(nodeFlowId(n))
  for (const z of h.zones) ids.add(zoneFlowId(z))
  for (const e of h.edges) {
    const edge = editor.edgeById.get(e)
    if (edge) { ids.add(nodeFlowId(edge.source)); ids.add(nodeFlowId(edge.target)) }
  }
  if (ids.size) void fitView({ nodes: [...ids], padding: 0.45, maxZoom: 1.1, duration: 400 })
})

watch(() => editor.reveal, (r) => {
  if (!r) return
  const ids = [...r.nodes.map(nodeFlowId), ...r.zones.map(zoneFlowId)]
  void fitView({ nodes: ids, padding: 0.7, maxZoom: 1, duration: 350 })
})

// ----------------------------------------------------------------------------------- selection
// Shift+click and Shift+drag (a rectangle) select several cards; dragging one of them moves them all.
const selectedCards = computed(() => getSelectedNodes.value.filter((n) => n.type === 'card').map((n) => (n.data as { id: string }).id))
watch(selectedCards, (ids) => editor.setMulti(ids))
watch(() => editor.selectRequest, (r) => {
  if (!r) return
  removeSelectedElements()
  const found = r.ids.map((id) => findNode(nodeFlowId(id))).filter((n): n is GraphNode => !!n)
  if (found.length) addSelectedNodes(found)
})
watch(() => editor.multi.length, (n) => { if (!n && getSelectedNodes.value.length > 1) removeSelectedElements() })

const zoomPct = computed(() => Math.round((viewport.value.zoom || 1) * 100))
const lod = computed(() => lodOf(viewport.value.zoom || 1, app.editorCfg))

// ------------------------------------------------------------------------------------ interaction
function onDragStop(ev: NodeDragEvent) {
  const moves = { nodes: {} as Record<string, { x: number; y: number }>, zones: {} as Record<string, { x: number; y: number }> }
  let any = false
  for (const n of ev.nodes) {
    const { x, y } = n.computedPosition
    if (n.id.startsWith('z:')) {
      const id = n.id.slice(2)
      const z = editor.zoneById.get(id)
      if (z && (Math.abs(x - (z.ui?.x ?? 0)) > 0.5 || Math.abs(y - (z.ui?.y ?? 0)) > 0.5)) { moves.zones[id] = { x, y }; any = true }
    } else if (n.id.startsWith('n:')) {
      const id = n.id.slice(2)
      const nd = editor.nodeById.get(id)
      if (nd && (Math.abs(x - (nd.ui?.x ?? 0)) > 0.5 || Math.abs(y - (nd.ui?.y ?? 0)) > 0.5)) { moves.nodes[id] = { x, y }; any = true }
    }
  }
  if (any) editor.commitMoves(moves)
}

function onPaneClick() {
  if (editor.tool === 'connect') editor.connectFrom = null
  editor.select(null)
  if (editor.pickingNode) editor.pickingNode = false
}

function onConnect(c: { source: string; target: string }) {
  if (c.source.startsWith('n:') && c.target.startsWith('n:')) editor.connect(c.source.slice(2), c.target.slice(2))
}

function onEdgeClick(e: { edge: { id: string } }) {
  editor.select({ type: 'edge', id: e.edge.id.slice(2) })
}

function onDrop(e: DragEvent) {
  const raw = e.dataTransfer?.getData('application/x-infra-item')
  if (!raw || editor.readonly) return
  e.preventDefault()
  try {
    const item = JSON.parse(raw) as { type: 'kind' | 'zone'; id: string }
    const p = screenToFlowCoordinate({ x: e.clientX, y: e.clientY })
    if (item.type === 'kind') editor.addNode(item.id, p)
    else editor.addZone(item.id, { x: p.x - 260, y: p.y - 40, w: 520, h: 420 })
  } catch { /* foreign drag payload */ }
}

function onDragOver(e: DragEvent) {
  if (e.dataTransfer?.types.includes('application/x-infra-item')) { e.preventDefault(); e.dataTransfer.dropEffect = 'copy' }
}

// ------------------------------------------------------------------------------------ zone tool
const drawing = ref<{ x0: number; y0: number; x1: number; y1: number } | null>(null)
const zoneTypeOptions = computed(() => app.boot?.catalog.zoneTypes ?? [])
const drawZoneType = ref(zoneTypeOptions.value.find((z) => z.level === 'site')?.id ?? zoneTypeOptions.value[0]?.id ?? '')

function localPoint(e: PointerEvent) {
  const r = wrap.value!.getBoundingClientRect()
  return { x: e.clientX - r.left, y: e.clientY - r.top }
}

function drawStart(e: PointerEvent) {
  const p = localPoint(e)
  drawing.value = { x0: p.x, y0: p.y, x1: p.x, y1: p.y }
  ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
}
function drawMove(e: PointerEvent) {
  if (!drawing.value) return
  const p = localPoint(e)
  drawing.value = { ...drawing.value, x1: p.x, y1: p.y }
}
function drawEnd(e: PointerEvent) {
  const d = drawing.value
  drawing.value = null
  if (!d) return
  const r = wrap.value!.getBoundingClientRect()
  const a = screenToFlowCoordinate({ x: r.left + Math.min(d.x0, d.x1), y: r.top + Math.min(d.y0, d.y1) })
  const b = screenToFlowCoordinate({ x: r.left + Math.max(d.x0, d.x1), y: r.top + Math.max(d.y0, d.y1) })
  const w = b.x - a.x
  const h = b.y - a.y
  if (w > 60 && h > 60) {
    editor.addZone(drawZoneType.value, { x: a.x, y: a.y, w: Math.max(w, MIN_ZONE_W), h: Math.max(h, MIN_ZONE_H) })
    editor.setTool('select')
  } else if (w < 8 && h < 8) {
    editor.addZone(drawZoneType.value, { x: a.x - 260, y: a.y - 30, w: 520, h: 420 })
    editor.setTool('select')
  }
  void e
}
const drawRect = computed(() => {
  const d = drawing.value
  return d ? { left: Math.min(d.x0, d.x1) + 'px', top: Math.min(d.y0, d.y1) + 'px', width: Math.abs(d.x1 - d.x0) + 'px', height: Math.abs(d.y1 - d.y0) + 'px' } : null
})

// ---------------------------------------------------------------------------------------- toolbar
const layers: Layer[] = ['network', 'data', 'dr']
function toggleLayer(l: Layer) {
  editor.layer = editor.layer === l ? 'all' : l
}
const legend = computed(() => (app.boot?.catalog.edgeTypes ?? []).map((e) => ({ id: e.id, label: lt(e.short ?? e.name), color: e.style.color, dash: e.style.dash ?? '', width: Math.max(1.5, e.style.width ?? 1.6) })))
const hint = computed(() => (editor.tool === 'connect' ? t('canvas.connect.hint') : editor.tool === 'zone' ? t('canvas.zone.hint') : editor.pickingNode ? t('sim.node.pick') : ''))

function nodeColor(n: { id: string; type?: string }) {
  if (n.type === 'zone') return '#e4e9f2'
  const node = editor.nodeById.get(n.id.slice(2))
  return (node && app.index?.kinds.get(node.kind)?.color) || '#8b93a1'
}
</script>

<template>
  <div ref="wrap" class="canvas" :class="['tool-' + editor.tool, editor.pickingNode && 'picking']" :data-lod="lod" :style="{ '--zoom': viewport.zoom || 1 }" @dragover="onDragOver" @drop="onDrop">
    <VueFlow
      :nodes="flow.nodes" :edges="flow.edges" :node-types="nodeTypes" :edge-types="edgeTypes"
      :min-zoom="0.15" :max-zoom="1.8" :default-viewport="{ x: 40, y: 40, zoom: 0.8 }"
      :nodes-draggable="!editor.readonly" :nodes-connectable="!editor.readonly" :elements-selectable="true"
      :connection-mode="ConnectionMode.Loose" :connection-radius="30" :delete-key-code="null" :selection-key-code="'Shift'" :multi-selection-key-code="'Shift'"
      :pan-on-drag="editor.tool !== 'zone'" :zoom-on-double-click="false" :prevent-scrolling="true" :snap-to-grid="false" :auto-pan-on-node-drag="true"
      @node-drag-stop="onDragStop" @pane-click="onPaneClick" @connect="onConnect" @edge-click="onEdgeClick" @move-end="updateCenter"
    >
      <Background :gap="22" :size="1.6" />
      <MiniMap class="minimap" pannable zoomable :node-color="nodeColor" :node-stroke-width="0" mask-color="rgba(20,23,31,.06)" />
    </VueFlow>

    <div v-if="editor.tool === 'zone'" class="draw-layer" @pointerdown="drawStart" @pointermove="drawMove" @pointerup="drawEnd">
      <div v-if="drawRect" class="draw-rect" :style="drawRect" />
    </div>

    <button v-if="!ui.libraryOpen" class="open-lib" :title="t('library.expand')" :aria-label="t('library.expand')" @click="ui.libraryOpen = true"><PanelLeftOpen :size="16" /></button>
    <div class="toolbar" :class="{ shifted: !ui.libraryOpen }">
      <template v-if="app.canEdit">
        <button class="tool" :class="{ on: editor.tool === 'select' }" @click="editor.setTool('select')"><MousePointer2 :size="14" />{{ t('tool.select') }}</button>
        <button class="tool" :class="{ on: editor.tool === 'connect' }" :disabled="editor.readonly" @click="editor.setTool('connect')"><Spline :size="14" />{{ t('tool.connect') }}</button>
        <button class="tool" :class="{ on: editor.tool === 'zone' }" :disabled="editor.readonly" @click="editor.setTool('zone')"><Square :size="14" />{{ t('tool.zone') }}</button>
        <select v-if="editor.tool === 'zone'" v-model="drawZoneType" class="select input-sm zsel" :aria-label="t('inspector.zone.type')">
          <option v-for="z in zoneTypeOptions" :key="z.id" :value="z.id">{{ lt(z.name) }}</option>
        </select>
        <button class="tool" :disabled="editor.readonly || editor.isEmpty" @click="editor.autoLayout()"><LayoutDashboard :size="14" />{{ t('tool.layout') }}</button>
      </template>
      <span class="layers">
        <span class="lbl">{{ t('tool.layers') }}:</span>
        <button v-for="l in layers" :key="l" class="lay" :class="{ on: editor.layer === l }" @click="toggleLayer(l)">{{ t('layer.' + l) }}</button>
      </span>
      <span class="legend">
        <span v-for="l in legend" :key="l.id" class="leg">
          <svg width="22" height="8" aria-hidden="true"><line x1="0" y1="4" x2="22" y2="4" :stroke="l.color" :stroke-width="l.width" :stroke-dasharray="l.dash" /></svg>{{ l.label }}
        </span>
      </span>
    </div>
    <div v-if="hint" class="hint-bar">{{ hint }}</div>

    <div class="zoom">
      <template v-if="app.canEdit">
        <button :disabled="!editor.canUndo" :title="t('tool.undo') + ' (Ctrl+Z)'" :aria-label="t('tool.undo')" @click="editor.undo()"><Undo2 :size="14" /></button>
        <button :disabled="!editor.canRedo" :title="t('tool.redo') + ' (Ctrl+Shift+Z)'" :aria-label="t('tool.redo')" class="redo" @click="editor.redo()"><Redo2 :size="14" /></button>
      </template>
      <button :aria-label="'−'" @click="zoomOut({ duration: 120 })"><Minus :size="14" /></button>
      <span class="pct">{{ zoomPct }}%</span>
      <button :aria-label="'+'" @click="zoomIn({ duration: 120 })"><Plus :size="14" /></button>
      <button class="fit" @click="fit()">{{ t('canvas.fit') }}</button>
    </div>

    <div v-if="editor.isEmpty && !editor.loading && !editor.preview" class="empty-state">
      <h3>{{ t('canvas.empty.title') }}</h3>
      <p>{{ t('canvas.empty.text') }}</p>
    </div>
  </div>
</template>

<style>
.canvas { position: absolute; inset: 0; background: var(--bg); overflow: hidden; }
.canvas .vue-flow { background: transparent; }
.canvas .vue-flow__background { background: transparent; }
.canvas .vue-flow__background pattern circle, .canvas .vue-flow__background circle { fill: var(--grid-dot); }
.canvas .vue-flow__node-zone { pointer-events: none !important; }
.canvas .vue-flow__node-zone .zone-head, .canvas .vue-flow__node-zone .zone-resize { pointer-events: auto; }
.canvas .vue-flow__handle { z-index: 40; }
.canvas .vue-flow__edge-path { stroke-linecap: round; }
.canvas .vue-flow__connection-path { stroke: var(--accent); stroke-width: 2; stroke-dasharray: 5 4; }
.canvas .vue-flow__node { font-family: var(--font); }
/* semantic zoom (see lib/lod.ts): what stays legible when the diagram is zoomed out */
.canvas[data-lod='compact'] .arch-node .sub, .canvas[data-lod='mini'] .arch-node .sub { display: none; }
.canvas[data-lod='compact'] .arch-node .name { font-size: clamp(14px, calc(10.5px / var(--zoom, 1)), 26px); white-space: normal; line-height: 1.12; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; text-overflow: clip; }
.canvas[data-lod='compact'] .zone-head .zname, .canvas[data-lod='mini'] .zone-head .zname { font-size: clamp(14px, calc(11.5px / var(--zoom, 1)), 30px); }
.canvas[data-lod='compact'] .zone-head .ztag, .canvas[data-lod='mini'] .zone-head .ztag, .canvas[data-lod='compact'] .edge-label, .canvas[data-lod='mini'] .edge-label { display: none; }
.canvas[data-lod='mini'] .arch-node .txt { display: none; }
.canvas[data-lod='mini'] .arch-node .badge { position: absolute; inset: 0; width: auto; height: auto; border-radius: inherit; font-size: 26px; }
.canvas[data-lod='mini'] .arch-node .dot { width: 18px; height: 18px; top: -8px; right: -8px; z-index: 2; }
.canvas.tool-zone .vue-flow__pane { cursor: crosshair; }
.canvas.picking .vue-flow__pane { cursor: crosshair; }
.minimap.vue-flow__minimap { right: 18px; bottom: 18px; width: 170px; height: 118px; border: 1px solid var(--line-strong); border-radius: var(--radius); background: var(--surface); box-shadow: var(--shadow-1); margin: 0; overflow: hidden; }
@container (max-width: 1250px) { .minimap.vue-flow__minimap { display: none; } }
.draw-layer { position: absolute; inset: 0; z-index: 50; cursor: crosshair; }
.draw-rect { position: absolute; border: 1.5px dashed var(--accent); background: rgba(46, 91, 236, .06); border-radius: 12px; }

.open-lib { position: absolute; top: 14px; left: 14px; z-index: 61; width: 32px; height: 32px; display: inline-flex; align-items: center; justify-content: center; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); background: var(--surface); color: var(--text-2); }
.open-lib:hover { background: var(--surface-2); }
.toolbar.shifted { left: 56px; }
.toolbar { position: absolute; top: 14px; left: 16px; right: 16px; display: flex; align-items: center; gap: 6px; z-index: 60; flex-wrap: wrap; justify-content: center; pointer-events: none; }
.toolbar > * { pointer-events: auto; }
.tool { display: inline-flex; align-items: center; gap: 6px; height: 32px; padding: 0 12px; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); background: var(--surface); color: var(--text-2); font-size: 13px; }
.tool.icon { padding: 0; width: 32px; justify-content: center; }
.tool:hover:not(:disabled) { background: var(--surface-2); border-color: #b8c2d4; }
.tool:disabled { opacity: .45; cursor: not-allowed; }
.tool.on { background: var(--accent-soft); border-color: var(--accent-line); color: var(--accent); font-weight: 600; }
.toolbar .sep { width: 1px; height: 20px; background: var(--line-strong); margin: 0 4px; }
.zsel { width: 140px; }
.layers, .legend { display: inline-flex; align-items: center; gap: 4px; height: 32px; padding: 0 9px; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); background: var(--surface); font-size: 12.5px; color: var(--muted); }
.layers .lbl { margin-right: 2px; }
.lay { border: 0; background: transparent; padding: 2px 6px; border-radius: 4px; color: var(--text-2); font-size: 12.5px; }
.lay:hover { background: var(--surface-sunken); }
.lay.on { background: var(--accent-soft); color: var(--accent); font-weight: 600; }
.legend { gap: 11px; }
.leg { display: inline-flex; align-items: center; gap: 6px; white-space: nowrap; }
.hint-bar { position: absolute; top: 58px; left: 50%; transform: translateX(-50%); width: max-content; background: var(--ink); color: var(--on-dark); font-size: 12.5px; padding: 6px 12px; border-radius: var(--radius-sm); z-index: 60; pointer-events: none; }

.zoom { position: absolute; left: 18px; bottom: 18px; display: flex; align-items: stretch; height: 34px; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); background: var(--surface); overflow: hidden; z-index: 60; box-shadow: var(--shadow-1); }
.zoom button { border: 0; background: transparent; padding: 0 11px; color: var(--text-2); display: inline-flex; align-items: center; font-size: 13px; }
.zoom button:hover:not(:disabled) { background: var(--surface-2); }
.zoom button:disabled { opacity: .4; cursor: not-allowed; }
.zoom .redo { border-right: 1px solid var(--line); }
.zoom .fit { border-left: 1px solid var(--line); }
.zoom .pct { display: inline-flex; align-items: center; min-width: 52px; justify-content: center; font-size: 13px; color: var(--text-2); border-left: 1px solid var(--line); border-right: 1px solid var(--line); }

.empty-state { position: absolute; left: 50%; top: 50%; transform: translate(-50%, -50%); text-align: center; max-width: 420px; color: var(--muted); pointer-events: none; }
.empty-state h3 { font-size: 18px; color: var(--text); margin-bottom: 8px; }
.empty-state p { margin: 0; line-height: 1.55; }
</style>
