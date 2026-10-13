<script setup lang="ts">
import { computed } from 'vue'
import { Handle, Position, type NodeProps } from '@vue-flow/core'
import { Cloud } from 'lucide-vue-next'
import { lt, t } from '@/i18n'
import { nodeSubtitle, variantOf } from '@/lib/catalog'
import { isManaged } from '@/lib/product'
import { statusColor } from '@/lib/findings'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'
import { usePlayback } from '@/stores/playback'
import { SIDES } from './flowModel'

const props = defineProps<NodeProps>()
const app = useApp()
const editor = useEditor()
const playback = usePlayback()

const id = computed(() => props.data.id as string)
const node = computed(() => editor.nodeById.get(id.value))
const kind = computed(() => (node.value ? app.index?.kinds.get(node.value.kind) : undefined))
const subtitle = computed(() => (node.value && app.index ? nodeSubtitle(app.index, node.value) : ''))
const variant = computed(() => variantOf(kind.value, node.value?.props.variant))
/** A managed service is run by a provider: the card says so with a cloud, the tooltip names the offering. */
const managed = computed(() => isManaged(variant.value, kind.value))
const tip = computed(() => (proposed.value ? t('inspector.proposed') : kind.value?.variants?.length && variant.value ? `${lt(kind.value.name)} · ${lt(variant.value.name)}` : undefined))
const selected = computed(() => editor.selection?.type === 'node' && editor.selection.id === id.value)
const status = computed(() => editor.nodeStatus.get(id.value))
const simState = computed(() => (editor.sim?.result ? playback.nodeStates[id.value] : undefined))
/** The step on screen changed this card: it pulses once (two names, so the pulse restarts on the next step). */
const stepPulse = computed(() => (playback.changed.has(id.value) ? (playback.index % 2 ? 'step-b' : 'step-a') : ''))
const proposed = computed(() => !!editor.previewSets?.added.nodes.has(id.value))
const ghost = computed(() => !!editor.previewSets?.removed.nodes.has(id.value))
const changed = computed(() => !!editor.previewSets?.changed.nodes.has(id.value))
const lit = computed(() => !!editor.highlight?.nodes.includes(id.value))
const dim = computed(() => !!editor.highlight && !lit.value)
/** Something is selected and this card is not what it touches: it steps back (the links of the selection and the cards at their ends stay). */
const faded = computed(() => !!editor.focus && !editor.focus.nodes.has(id.value))
const connecting = computed(() => editor.connectFrom === id.value)
const inMulti = computed(() => editor.multi.includes(id.value))
/** The row of the first build this card belongs to: the cards of a freshly opened diagram appear row by row. */
const wave = computed(() => (typeof props.data.wave === 'number' ? props.data.wave : 0))

const dot = computed(() => {
  if (!editor.report || proposed.value) return proposed.value ? 'var(--info-dot)' : ''
  return status.value ? statusColor(status.value) : 'var(--ok-dot)'
})

const stateLabel = computed(() => (simState.value && simState.value !== 'up' ? t('node.state.' + simState.value) : ''))
const sides = { t: Position.Top, b: Position.Bottom, l: Position.Left, r: Position.Right }

// the click must reach the flow's node wrapper: that is what selects a card (Shift+click extends the selection)
function onClick(e: MouseEvent) {
  editor.clickNode(id.value, e)
}
</script>

<template>
  <div
    v-if="node"
    class="arch-node"
    :class="[
      selected && 'selected', inMulti && 'multi', status && !proposed && 'st-' + status, proposed && 'proposed', ghost && 'ghost', changed && 'changed', lit && 'lit', dim && 'dim', faded && 'faded', connecting && 'connecting',
      simState && 'sim-' + simState, stepPulse, editor.pickingNode && 'pickable', editor.tool === 'connect' && 'connectable',
    ]"
    :title="tip"
    :style="{ '--i': wave }"
    @click="onClick"
  >
    <span class="badge-abbr badge" :style="{ background: kind?.color ?? '#4c596c' }">{{ kind?.abbr ?? '?' }}</span>
    <div class="txt">
      <div class="name truncate">{{ node.name }}</div>
      <div class="sub truncate mono">{{ proposed && !subtitle ? t('inspector.proposed.short') : subtitle || (kind ? lt(kind.name) : t('kind.unknown', { kind: node.kind })) }}</div>
    </div>
    <span v-if="dot" class="dot" :style="{ background: dot }" />
    <span v-if="managed" class="mgd" aria-hidden="true"><Cloud :size="11" /></span>
    <span v-if="stateLabel" class="state-tag">{{ stateLabel }}</span>
    <template v-for="s in SIDES" :key="s">
      <Handle :id="s" type="source" :position="sides[s]" class="h" />
      <Handle :id="s" type="target" :position="sides[s]" class="h" />
    </template>
  </div>
</template>

<style scoped>
.arch-node {
  position: relative; width: 212px; height: 64px; display: flex; align-items: center; gap: 11px; padding: 0 14px 0 12px;
  background: var(--surface); border: 1px solid var(--line-strong); border-radius: var(--radius); box-shadow: var(--shadow-1);
  cursor: pointer; transition: box-shadow var(--dur-fast), border-color var(--dur-fast), opacity var(--dur-fast), background-color var(--dur-base);
  animation: pop-in var(--dur-base) var(--ease-spring) both; animation-delay: calc(min(var(--i, 0), 8) * var(--stagger) * 1.4);
}
.arch-node:hover { box-shadow: var(--shadow-2); }
.badge { width: 34px; height: 34px; border-radius: 7px; font-size: 10px; flex: none; }
.txt { min-width: 0; flex: 1; }
.name { font-weight: 600; font-size: 14px; line-height: 1.25; color: var(--text); }
.sub { font-size: 11.5px; color: var(--muted); line-height: 1.3; margin-top: 2px; }
.dot { position: absolute; top: -4px; right: -4px; width: 10px; height: 10px; border-radius: 50%; border: 2px solid var(--surface); box-sizing: content-box; }
.mgd { position: absolute; top: -8px; left: 10px; height: 16px; padding: 0 5px; display: inline-flex; align-items: center; border-radius: 8px; background: var(--surface); border: 1px solid var(--line-strong); color: var(--muted); }
.selected { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft), var(--shadow-2); }
.st-risk { border-color: var(--risk-dot); box-shadow: 0 0 0 3px var(--risk-bg), var(--shadow-1); }
.st-critical { border-color: var(--crit); box-shadow: 0 0 0 3px var(--crit-bg), var(--shadow-1); }
.selected.st-risk, .selected.st-critical { box-shadow: 0 0 0 3px var(--accent-soft), var(--shadow-2); border-color: var(--accent); }
.proposed { border: 1.5px dashed var(--info-dot); background: var(--accent-soft); }
.changed { border-color: var(--accent); }
.ghost { opacity: .5; border: 1.5px dashed var(--crit); background: var(--crit-bg); }
.ghost .name { text-decoration: line-through; }
.dim { opacity: .35; }
.faded { opacity: .5; }
.lit { border-color: var(--accent); box-shadow: 0 0 0 4px var(--accent-soft); }
.step-a::before, .step-b::before { content: ''; position: absolute; inset: -1px; border-radius: var(--radius); pointer-events: none; }
.step-a::before { animation: step-a var(--dur-slower) var(--ease-out) 1; }
.step-b::before { animation: step-b var(--dur-slower) var(--ease-out) 1; }
@keyframes step-a { from { box-shadow: 0 0 0 0 color-mix(in srgb, var(--accent) 50%, transparent); } to { box-shadow: 0 0 0 16px color-mix(in srgb, var(--accent) 0%, transparent); } }
@keyframes step-b { from { box-shadow: 0 0 0 0 color-mix(in srgb, var(--accent) 50%, transparent); } to { box-shadow: 0 0 0 16px color-mix(in srgb, var(--accent) 0%, transparent); } }
.lit::after { content: ''; position: absolute; inset: -1px; border-radius: var(--radius); pointer-events: none; animation: pulse-ring 1.1s ease-in-out 3; }
@keyframes pulse-ring { 50% { box-shadow: 0 0 0 9px color-mix(in srgb, var(--accent) 14%, transparent); } }
.connecting { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
.multi { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft), var(--shadow-2); }
.multi .h { opacity: 0 !important; }
.pickable { cursor: crosshair; }
.pickable:hover { border-color: var(--crit); box-shadow: 0 0 0 3px var(--crit-bg); }
.connectable { cursor: crosshair; }
.state-tag { position: absolute; left: 10px; bottom: -9px; padding: 0 7px; height: 18px; line-height: 18px; border-radius: 9px; font-size: 10.5px; font-weight: 700; letter-spacing: .03em; background: var(--ink); color: #fff; }
.sim-down { background: var(--crit-bg); border-color: var(--crit); }
.sim-down .name { color: var(--crit); }
.sim-down .state-tag { background: var(--crit); }
.sim-partial, .sim-stalled { background: var(--risk-bg); border-color: var(--risk-dot); }
.sim-partial .state-tag, .sim-stalled .state-tag { background: var(--risk); }
.sim-fenced, .sim-isolated { background: var(--nodata-bg); border-color: var(--faint); border-style: dashed; }
.sim-fenced .state-tag, .sim-isolated .state-tag { background: var(--nodata); }
.sim-promoted { background: var(--ok-bg); border-color: var(--ok-dot); }
.sim-promoted .state-tag { background: var(--ok); }
.h { opacity: 0; width: 11px; height: 11px; background: var(--accent); border: 2px solid #fff; transition: opacity var(--dur-fast); }
.arch-node:hover .h, .connectable .h, .connecting .h { opacity: 1; }
</style>
