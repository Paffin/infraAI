<script setup lang="ts">
import { computed, inject, ref } from 'vue'
import { Handle, Position, type NodeProps } from '@vue-flow/core'
import { Cloud } from 'lucide-vue-next'
import KindChip from '@/components/KindChip.vue'
import { lt, t } from '@/i18n'
import { nodeSubtitle, variantOf } from '@/lib/catalog'
import { isManaged } from '@/lib/product'
import { statusColor, statusLabel } from '@/lib/findings'
import { fmtNumber } from '@/lib/format'
import { LOD_KEY, type Lod } from '@/lib/lod'
import { longestWordEm } from '@/lib/textFit'
import { amountText, FLOW_EPS } from '@/lib/traffic'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'
import { usePlayback } from '@/stores/playback'
import { SIDES } from './flowModel'

const props = defineProps<NodeProps>()
const app = useApp()
const editor = useEditor()
const playback = usePlayback()
/** How far the diagram is zoomed out: what the card writes on itself, and so what its name has to contain, depends on it (see LOD_KEY). */
const lod = inject(LOD_KEY, ref<Lod>('full'))

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
/** Several instances: the card is drawn as a stack, one layer behind it for two, two for three or more. */
const instances = computed(() => {
  const r = node.value?.props.replicas
  return typeof r === 'number' && r > 1 ? r : 1
})
const inMulti = computed(() => editor.multi.includes(id.value))
/** The room the longest word of the name needs (ems): zoomed out, the name is drawn only as large as that word allows, so that no word is cut in the middle. */
const wordEm = computed(() => longestWordEm(node.value?.name ?? ''))
/** The row of the first build this card belongs to: the cards of a freshly opened diagram appear row by row. */
const wave = computed(() => (typeof props.data.wave === 'number' ? props.data.wave : 0))

const dot = computed(() => {
  if (!editor.report || proposed.value) return proposed.value ? 'var(--info-dot)' : ''
  return status.value ? statusColor(status.value) : 'var(--ok-dot)'
})

const stateLabel = computed(() => (simState.value && simState.value !== 'up' ? t('node.state.' + simState.value) : ''))

/** While a failure is shown: how the load on this card differs from normal operation (×2 = twice as much arrives at it). */
const load = computed(() => (editor.sim?.result && simState.value !== 'down' ? playback.loads[id.value] : undefined))
const loadTone = computed(() => {
  const l = load.value
  return !l ? '' : l.before <= FLOW_EPS ? 'new' : l.now <= FLOW_EPS ? 'none' : l.ratio > 1 ? 'up' : 'down'
})
const loadText = computed(() => {
  const l = load.value
  if (!l) return ''
  if (loadTone.value === 'new') return t('traffic.load.new')
  if (loadTone.value === 'none') return '—'
  return '×' + String(Math.round(l.ratio * 10) / 10)
})
const loadTip = computed(() => {
  const l = load.value
  if (!l || !node.value) return undefined
  const amount = (v: number) => {
    const a = amountText(v, editor.arch.requirements.rpsPeak, fmtNumber)
    return a.unit === 'rps' ? `${a.value} ${t('traffic.unit.rps')}` : `${a.value}%`
  }
  return l.now <= FLOW_EPS ? t('traffic.load.zero', { name: node.value.name, before: amount(l.before) }) : t('traffic.load.tip', { name: node.value.name, before: amount(l.before), now: amount(l.now) })
})
const sides = { t: Position.Top, b: Position.Bottom, l: Position.Left, r: Position.Right }

/** The second line of the card: what it is made of (the sub-line of the product), or the kind of it when that is empty, or that it is a proposal. */
const subLine = computed(() => (proposed.value && !subtitle.value ? t('inspector.proposed.short') : subtitle.value || (kind.value ? lt(kind.value.name) : t('kind.unknown', { kind: node.value?.kind ?? '' }))))
/**
 * What a screen reader and a voice control call the card. It contains the words that are written on the card, in the order they are written (WCAG 2.5.3; axe
 * checks it as `label-content-name-mismatch`): a voice control says "click Kafka", and a name that does not contain what is on the card makes that fail. What is
 * written depends on how far the diagram is zoomed out (lib/lod.ts): the name, the sub-line, the state and the load tags of a failure on screen; at `compact` the
 * sub-line is gone; at `mini` the name is gone too and only the abbreviation of the kind stays (then the name comes first, so that a name is what a screen reader
 * hears first, and the written words follow it as one run). What this level leaves off follows after a comma, then what the picture says without words (the finding it
 * carries, the instances, a proposal) and what the tags mean.
 */
const label = computed(() => {
  const n = node.value
  if (!n) return undefined
  const mini = lod.value === 'mini'
  const full = lod.value === 'full'
  const tags = [stateLabel.value, loadText.value]
  const written = (mini ? [kind.value?.abbr ?? '?', ...tags] : full ? [n.name, subLine.value, ...tags] : [n.name, ...tags]).filter(Boolean).join(' ')
  const extras = [
    full ? '' : subLine.value,
    status.value && !proposed.value ? statusLabel(status.value).toLowerCase() : '',
    instances.value > 1 ? t('card.instances', { n: instances.value }) : '',
    stateLabel.value ? t('card.sim') : '',
    loadTone.value === 'up' ? t('card.load.up') : loadTone.value === 'down' ? t('card.load.down') : loadTone.value === 'none' ? t('card.load.none') : '',
    proposed.value && subLine.value !== t('inspector.proposed.short') ? t('inspector.proposed.short') : '',
    ghost.value ? t('card.removed') : '',
    connecting.value ? t('card.connecting') : '',
  ].filter(Boolean)
  return [mini ? n.name : '', written, ...extras].filter(Boolean).join(', ')
})

// the click must reach the flow's node wrapper: that is what selects a card (Shift+click extends the selection); the keys of the card are the canvas's (Canvas.vue)
function onClick(e: MouseEvent) {
  editor.clickNode(id.value, e)
}
</script>

<template>
  <div
    v-if="node"
    class="arch-node"
    :class="[
      instances > 1 && (instances > 2 ? 'stack-3' : 'stack-2'), selected && 'selected', inMulti && 'multi', status && !proposed && 'st-' + status, proposed && 'proposed', ghost && 'ghost', changed && 'changed', lit && 'lit', dim && 'dim', faded && 'faded', connecting && 'connecting',
      simState && 'sim-' + simState, stepPulse, editor.pickingNode && 'pickable', editor.tool === 'connect' && 'connectable',
    ]"
    :title="tip"
    :style="{ '--i': wave, '--wem': wordEm }"
    role="button" tabindex="-1" data-kbd="node" :data-id="id" :aria-pressed="selected || inMulti" :aria-label="label"
    @click="onClick"
  >
    <KindChip class="chip" :kind="kind" :size="38" />
    <!-- the space between the two lines is in the text, not only in the layout: a reader that joins the text of blocks without one reads "NameSub-line" -->
    <div class="txt">
      <div class="name truncate">{{ node.name }}</div>{{ ' ' }}
      <div class="sub truncate mono">{{ subLine }}</div>
    </div>
    <span v-if="dot" class="dot" :style="{ background: dot }" />
    <span v-if="managed" class="mgd" aria-hidden="true"><Cloud :size="11" /></span>
    <!-- the tags of a failure on screen are written on the card, so they are the first words of its label too; the spaces keep the words of a reader apart -->
    {{ ' ' }}<span v-if="stateLabel" class="state-tag">{{ stateLabel }}</span>
    {{ ' ' }}<span v-if="loadText" class="load-tag" :class="loadTone" :title="loadTip">{{ loadText }}</span>
    <template v-for="s in SIDES" :key="s">
      <Handle :id="s" type="source" :position="sides[s]" class="h" />
      <Handle :id="s" type="target" :position="sides[s]" class="h" />
    </template>
  </div>
</template>

<style scoped>
/* The card's shadows are three layers kept in variables, so that a state changes one of them and keeps the others: the ring (selection,
   findings), the stack (several instances: layers peek out under the card) and the lift. */
.arch-node {
  --ring: 0 0 0 0 transparent; --deck: 0 0 0 0 transparent; --lift: 0 1px 1px rgba(20, 23, 31, .04), 0 3px 10px rgba(20, 23, 31, .06); --edge: color-mix(in srgb, var(--line-strong) 85%, #fff);
  position: relative; width: 212px; height: 64px; display: flex; align-items: center; gap: 11px; padding: 0 12px;
  background: var(--surface); border: 1px solid color-mix(in srgb, var(--line-strong) 80%, #fff); border-radius: 11px;
  box-shadow: var(--ring), var(--deck), var(--lift);
  cursor: pointer; transition: box-shadow var(--dur-fast), border-color var(--dur-fast), opacity var(--dur-fast), background-color var(--dur-base);
  animation: pop-in var(--dur-base) var(--ease-spring) both; animation-delay: calc(min(var(--i, 0), 8) * var(--stagger) * 1.4);
}
.arch-node:hover { --lift: 0 1px 1px rgba(20, 23, 31, .05), 0 8px 22px rgba(20, 23, 31, .11); border-color: var(--line-strong); }
.stack-2 { --deck: 0 4px 0 -3px var(--surface), 0 5px 0 -3px var(--edge); }
.stack-3 { --deck: 0 4px 0 -3px var(--surface), 0 5px 0 -3px var(--edge), 0 9px 0 -6px var(--surface), 0 10px 0 -6px var(--edge); }
.txt { min-width: 0; flex: 1; }
.name { font-weight: 620; font-size: 13.5px; line-height: 1.25; color: var(--text); letter-spacing: -.005em; }
.sub { font-size: 11px; color: var(--muted); line-height: 1.3; margin-top: 3px; }
.dot { position: absolute; top: -4px; right: -4px; width: 10px; height: 10px; border-radius: 50%; border: 2px solid var(--surface); box-sizing: content-box; box-shadow: 0 1px 3px rgba(20, 23, 31, .25); }
.mgd { position: absolute; top: -8px; left: 11px; height: 16px; padding: 0 6px; display: inline-flex; align-items: center; border-radius: 8px; background: var(--surface); border: 1px solid var(--line-strong); color: var(--muted); }
.selected { border-color: var(--accent); --ring: 0 0 0 3px var(--accent-soft); --lift: var(--shadow-2); }
.st-risk { border-color: var(--risk-dot); --ring: 0 0 0 3px var(--risk-bg); }
.st-critical { border-color: var(--crit); --ring: 0 0 0 3px var(--crit-bg); }
.selected.st-risk, .selected.st-critical { --ring: 0 0 0 3px var(--accent-soft); --lift: var(--shadow-2); border-color: var(--accent); }
.proposed { border: 1.5px dashed var(--info-dot); background: var(--accent-soft); }
.changed { border-color: var(--accent); }
.ghost { opacity: .5; border: 1.5px dashed var(--crit); background: var(--crit-bg); }
.ghost .name { text-decoration: line-through; }
.dim { opacity: .35; }
.faded { opacity: .5; }
.lit { border-color: var(--accent); --ring: 0 0 0 4px var(--accent-soft); }
.step-a::before, .step-b::before { content: ''; position: absolute; inset: -1px; border-radius: 11px; pointer-events: none; }
.step-a::before { animation: step-a var(--dur-slower) var(--ease-out) 1; }
.step-b::before { animation: step-b var(--dur-slower) var(--ease-out) 1; }
@keyframes step-a { from { box-shadow: 0 0 0 0 color-mix(in srgb, var(--accent) 50%, transparent); } to { box-shadow: 0 0 0 16px color-mix(in srgb, var(--accent) 0%, transparent); } }
@keyframes step-b { from { box-shadow: 0 0 0 0 color-mix(in srgb, var(--accent) 50%, transparent); } to { box-shadow: 0 0 0 16px color-mix(in srgb, var(--accent) 0%, transparent); } }
.lit::after { content: ''; position: absolute; inset: -1px; border-radius: 11px; pointer-events: none; animation: pulse-ring 1.1s ease-in-out 3; }
@keyframes pulse-ring { 50% { box-shadow: 0 0 0 9px color-mix(in srgb, var(--accent) 14%, transparent); } }
.connecting { border-color: var(--accent); --ring: 0 0 0 3px var(--accent-soft); }
.multi { border-color: var(--accent); --ring: 0 0 0 3px var(--accent-soft); --lift: var(--shadow-2); }
.multi .h { opacity: 0 !important; }
.pickable { cursor: crosshair; }
.pickable:hover { border-color: var(--crit); --ring: 0 0 0 3px var(--crit-bg); }
.connectable { cursor: crosshair; }
.state-tag { position: absolute; left: 10px; bottom: -9px; padding: 0 7px; height: 18px; line-height: 18px; border-radius: 9px; font-size: 10.5px; font-weight: 700; letter-spacing: .03em; background: var(--ink); color: #fff; }
.load-tag { position: absolute; right: 10px; bottom: -9px; padding: 0 7px; height: 18px; line-height: 18px; border-radius: 9px; font-size: 10.5px; font-weight: 700; letter-spacing: .02em; font-variant-numeric: tabular-nums; background: var(--surface); border: 1px solid var(--line-strong); color: var(--text-2); animation: pop-in var(--dur-base) var(--ease-spring) both; }
.load-tag.up { background: var(--risk-bg); border-color: var(--risk-line); color: var(--risk); }
.load-tag.new { background: color-mix(in srgb, var(--flow-detour) 10%, #fff); border-color: color-mix(in srgb, var(--flow-detour) 40%, #fff); color: #0f6b63; }
.load-tag.none { color: var(--muted); }
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
