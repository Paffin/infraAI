<script setup lang="ts">
import { computed, ref } from 'vue'
import { BaseEdge, EdgeLabelRenderer, getBezierPath, getStraightPath, Position, useVueFlow, type EdgeProps } from '@vue-flow/core'
import { X } from 'lucide-vue-next'
import { lt, t } from '@/i18n'
import { fmtNumber } from '@/lib/format'
import { labelSize, pickLabelSpot, type Box, type Spot } from '@/lib/labelSpot'
import { amountText, flowChange, flowWidth, FLOW_EPS } from '@/lib/traffic'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'
import { usePlayback } from '@/stores/playback'
import FlowParticles from './FlowParticles.vue'
import { CARD_H, CARD_W } from './layout'

const props = defineProps<EdgeProps>()

/** A path nobody sees, to ask where a point of a link is (the label goes there). */
let probe: SVGPathElement | null = null
function pathProbe(): SVGPathElement | null {
  if (typeof document === 'undefined') return null
  if (!probe) {
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg')
    svg.setAttribute('aria-hidden', 'true')
    svg.style.cssText = 'position:absolute;width:0;height:0;overflow:hidden;visibility:hidden;pointer-events:none'
    probe = document.createElementNS('http://www.w3.org/2000/svg', 'path')
    svg.appendChild(probe)
    document.body.appendChild(svg)
  }
  return probe
}
/** The boxes of the cards, shared by every link of the diagram: they change when the design does. */
const boxes = new WeakMap<object, Box[]>()
function cardBoxes(nodes: { ui?: { x?: number; y?: number } | null }[]): Box[] {
  let b = boxes.get(nodes)
  if (!b) {
    b = nodes.filter((n) => Number.isFinite(n.ui?.x) && Number.isFinite(n.ui?.y)).map((n) => ({ x: n.ui!.x!, y: n.ui!.y!, w: CARD_W, h: CARD_H }))
    boxes.set(nodes, b)
  }
  return b
}
const app = useApp()
const editor = useEditor()
const playback = usePlayback()
const { viewport } = useVueFlow()

const id = computed(() => props.data.id as string)
const edge = computed(() => editor.edgeById.get(id.value))
const def = computed(() => (edge.value ? app.index?.edgeTypes.get(edge.value.type) : undefined))
const selected = computed(() => editor.selection?.type === 'edge' && editor.selection.id === id.value)
const simState = computed(() => (editor.sim?.result ? playback.edgeStates[id.value] : undefined))
const status = computed(() => editor.edgeStatus.get(id.value))
const added = computed(() => !!editor.previewSets?.added.edges.has(id.value))
const ghost = computed(() => !!editor.previewSets?.removed.edges.has(id.value))
const lit = computed(() => !!editor.highlight?.edges.includes(id.value))
/** What the selection touches (`emph`), and the links that are not part of it while something is selected (`faded`). */
const emph = computed(() => !!editor.focus?.edges.has(id.value))
const faded = computed(() => !!editor.focus && !editor.focus.edges.has(id.value))
/** A link between two sites: a design of many sites quiets these when zoomed out (CSS, `data-dense`), so that the sites themselves read. */
const cross = computed(() => props.data.cross === true)

// ---- the traffic: what the engine says this link carries in the picture on screen (only requests go along dependency links)
const traffic = computed(() => (editor.sim?.result && def.value?.semantics === 'dependency' ? playback.traffic : null))
const flow = computed(() => traffic.value?.flows[id.value] ?? 0)
const lostFlow = computed(() => traffic.value?.lost?.[id.value] ?? 0)
const missedFlow = computed(() => traffic.value?.missed?.[id.value] ?? 0)
const carrying = computed(() => flow.value > FLOW_EPS)
/** Something sent into the link is dropped on it: the requests behind it fail (`losing`), or only a call to an optional dependency goes unanswered (`missing`). */
const losing = computed(() => lostFlow.value > FLOW_EPS)
const missing = computed(() => missedFlow.value > FLOW_EPS)
const dropping = computed(() => losing.value || missing.value)
/** In this picture nothing goes along the link (it is not dead: simply nobody sends anything). */
const idle = computed(() => !!traffic.value && !carrying.value && !dropping.value && simState.value === 'ok')
const flowBefore = computed(() => playback.before?.flows[id.value] ?? 0)
const change = computed(() => (traffic.value ? flowChange(flowBefore.value, flow.value) : null))
const cutLink = computed(() => simState.value === 'cut')
/** How much is dropped: what the red (or amber) band is as wide as. */
const droppedFlow = computed(() => (losing.value ? lostFlow.value : missedFlow.value))
/** A line stays readable when the diagram is zoomed far out: it is never thinner than this on the screen. */
const minWidth = computed(() => 1.5 / Math.max(0.15, Math.round((viewport.value.zoom || 1) * 10) / 10))

const color = computed(() => {
  if (carrying.value) return 'var(--flow-band)'
  if (losing.value) return 'var(--flow-lost-band)'
  if (missing.value) return 'var(--flow-missed-band)'
  return cutLink.value ? 'var(--crit)' : simState.value === 'dead' ? 'var(--faint)' : def.value?.style.color ?? 'var(--edge-traffic)'
})
/** The colour of the arrowhead: the band itself is pale, the head is not. */
const headColor = computed(() => (carrying.value ? 'var(--flow)' : losing.value ? 'var(--flow-lost)' : missing.value ? 'var(--flow-missed)' : color.value))
const width = computed(() => {
  const pick = selected.value || lit.value ? 1.4 : emph.value ? 1.1 : 0
  if (traffic.value && (carrying.value || dropping.value)) return Math.max(minWidth.value, flowWidth(carrying.value ? flow.value : droppedFlow.value, playback.flowRef)) + pick
  return (def.value?.style.width ?? 1.6) + pick
})
const dash = computed(() => (dropping.value && !carrying.value ? '6 5' : cutLink.value ? '5 4' : ghost.value ? '4 4' : def.value?.style.dash ?? undefined))
const dimmed = computed(() => {
  if (editor.highlight && !lit.value) return true
  if (editor.layer !== 'all' && (def.value?.layer ?? 'network') !== editor.layer) return true
  return simState.value === 'dead' && !dropping.value
})

const path = computed(() => {
  const lane = ((props.data.lane as number) - ((props.data.lanes as number) - 1) / 2) * 16
  const horizontal = props.data.horizontal as boolean
  const sx = props.sourceX + (horizontal ? 0 : lane)
  const sy = props.sourceY + (horizontal ? lane : 0)
  const tx = props.targetX + (horizontal ? 0 : lane)
  const ty = props.targetY + (horizontal ? lane : 0)
  if (horizontal && Math.abs(sy - ty) < 2) {
    const [p, lx, ly] = getStraightPath({ sourceX: sx, sourceY: sy, targetX: tx, targetY: ty })
    return { d: p, lx, ly }
  }
  const [p, lx, ly] = getBezierPath({ sourceX: sx, sourceY: sy, sourcePosition: props.sourcePosition ?? Position.Bottom, targetX: tx, targetY: ty, targetPosition: props.targetPosition ?? Position.Top, curvature: 0.32 })
  return { d: p, lx, ly }
})
/** How long the link is, near enough to space particles by (a curve is a little longer than the straight line). */
const length = computed(() => Math.hypot(props.targetX - props.sourceX, props.targetY - props.sourceY) * 1.1 + 12)

const label = computed(() => {
  const e = edge.value
  if (!e) return ''
  const parts: string[] = []
  if (e.label) return e.label
  if (def.value && def.value.semantics !== 'dependency') parts.push(e.type)
  const rtt = e.props.latencyMs
  if (typeof rtt === 'number') parts.push(`RTT ${rtt} ${t('unit.ms')}`)
  return parts.join(' · ')
})

const hover = ref(false)
const long = computed(() => {
  const len = Math.hypot(props.targetX - props.sourceX, props.targetY - props.sourceY)
  return len >= label.value.length * 6.6 + 40
})
const quiet = computed(() => def.value?.semantics === 'control')

/** While a failure is shown, a link says what it carries when that is not what it carried before, and what it loses. */
const amount = (v: number) => {
  const a = amountText(v, editor.arch.requirements.rpsPeak, fmtNumber)
  return a.unit === 'rps' ? `${a.value} ${t('traffic.unit.rps')}` : `${a.value}%`
}
const flowLabel = computed(() => {
  if (!traffic.value) return ''
  if (dropping.value) {
    const gone = losing.value ? t('traffic.edge.lost', { amount: amount(lostFlow.value) }) : t('traffic.edge.missed', { amount: amount(missedFlow.value) })
    return carrying.value ? `${amount(flow.value)} · ${gone}` : gone
  }
  if (!carrying.value) return ''
  if (change.value && change.value !== 'new') return t('traffic.edge.change', { before: amount(flowBefore.value), now: amount(flow.value) })
  return hover.value || selected.value ? amount(flow.value) : ''
})
const flowTone = computed(() => (losing.value ? 'lost' : missing.value ? 'missed' : change.value === 'up' ? 'up' : change.value === 'down' ? 'down' : 'flat'))
/** A label on a short link would sit on the cards at its ends: it is shown there only when asked for (hover, selection) or when something is dropped on the link. */
const roomy = computed(() => Math.hypot(props.targetX - props.sourceX, props.targetY - props.sourceY) >= 130)
const showFlowLabel = computed(() => !!flowLabel.value && (hover.value || selected.value || dropping.value || (roomy.value && flowSpot.value.clear)))
/** While a failure is shown, the links that carry no requests (replication, quorum, backup, control) step back, so that the traffic reads. */
const stepBack = computed(() => !!editor.sim?.result && !!def.value && def.value.semantics !== 'dependency' && simState.value === 'ok')
const showLabel = computed(() => !!label.value && !traffic.value && (selected.value || hover.value || lit.value || emph.value || (long.value && !quiet.value && !faded.value && labelSpot.value.clear)))
/** A cut link: where it is cut, so that the eye finds the break. */
const showBreak = computed(() => cutLink.value && (dropping.value || flowBefore.value > FLOW_EPS))

/** Where a label of this link sits: the nearest spot to the middle that is clear of the cards. */
function spotFor(text: string): Spot {
  const mid = { x: path.value.lx, y: path.value.ly, clear: true }
  const el = text ? pathProbe() : null
  if (!el) return mid
  el.setAttribute('d', path.value.d)
  const total = el.getTotalLength()
  if (!(total > 0)) return mid
  return pickLabelSpot((f) => { const p = el.getPointAtLength(total * f); return { x: p.x, y: p.y } }, labelSize(text), cardBoxes(editor.shown.nodes))
}
const labelSpot = computed(() => spotFor(label.value))
const flowSpot = computed(() => spotFor(flowLabel.value))

const markerId = computed(() => `mk-${props.id}`)
const arrow = computed(() => def.value?.directed !== false)
</script>

<template>
  <g v-if="edge" :class="{ dim: dimmed, quiet: quiet && !selected && !hover && !emph, faded: faded && !hover, emph, cross, traffic: !!traffic, idle, carrying, dropping: losing, missing, 'step-back': stepBack }" @mouseenter="hover = true" @mouseleave="hover = false">
    <defs v-if="arrow">
      <marker :id="markerId" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="8" markerHeight="8" orient="auto-start-reverse" markerUnits="userSpaceOnUse">
        <path d="M0,1 L9,5 L0,9 Z" :fill="headColor" />
      </marker>
    </defs>
    <BaseEdge
      :id="id" :path="path.d" :marker-end="arrow ? `url(#${markerId})` : undefined" :interaction-width="20"
      :style="{ stroke: color, strokeWidth: width, strokeDasharray: dash, opacity: ghost ? 0.5 : 1 }"
      :class="{ 'edge-added': added, 'edge-risk': status === 'risk' || status === 'critical', 'edge-traffic': !!traffic }"
    />
    <!-- part of what is sent into the link is dropped on it: a red band over the pale one -->
    <path v-if="carrying && dropping" :d="path.d" class="lost-band" :class="{ soft: !losing }" fill="none" :style="{ strokeWidth: Math.max(minWidth, flowWidth(droppedFlow, playback.flowRef, 1.2, 2.2)) }" />
    <FlowParticles v-if="carrying || dropping" :d="path.d" :length="length" :flow="flow" :lost="lostFlow" :missed="missedFlow" :cut="cutLink" />
  </g>
  <EdgeLabelRenderer v-if="edge && showLabel">
    <div
      class="edge-label nodrag nopan" :class="{ selected, dim: dimmed, added }" @mouseenter="hover = true" @mouseleave="hover = false"
      :style="{ transform: `translate(-50%, -50%) translate(${labelSpot.x}px, ${labelSpot.y}px)`, '--ec': color }"
      :title="def ? lt(def.name) : edge.type"
      @click.stop="editor.select({ type: 'edge', id })"
    >{{ label }}</div>
  </EdgeLabelRenderer>
  <EdgeLabelRenderer v-if="edge && showFlowLabel">
    <div class="edge-label flow nodrag nopan" :class="flowTone" :style="{ transform: `translate(-50%, -50%) translate(${flowSpot.x}px, ${flowSpot.y}px)` }" @mouseenter="hover = true" @mouseleave="hover = false">{{ flowLabel }}</div>
  </EdgeLabelRenderer>
  <EdgeLabelRenderer v-if="edge && showBreak">
    <div class="edge-break nodrag nopan" :style="{ transform: `translate(-50%, -50%) translate(${path.lx}px, ${path.ly}px)` }" :title="t('node.state.down')"><X :size="11" :stroke-width="3" /></div>
  </EdgeLabelRenderer>
</template>

<style>
.edge-label {
  position: absolute; pointer-events: all; cursor: pointer; font-family: var(--font); font-weight: 500; font-size: 11px; line-height: 1; padding: 4px 9px; border-radius: 999px; letter-spacing: .005em; font-variant-numeric: tabular-nums;
  background: #fff; border: 1px solid color-mix(in srgb, var(--ec) 38%, var(--line-strong)); color: color-mix(in srgb, var(--ec) 72%, #000); white-space: nowrap; z-index: 30; box-shadow: var(--shadow-1);
}
.edge-label.selected { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
.edge-label.dim { opacity: .25; }
.edge-label.added { border-style: dashed; }
/* what a link carries (or loses) in the picture of a failure on screen */
.edge-label.flow { --ec: var(--flow); cursor: default; pointer-events: none; font-family: var(--font); font-weight: 600; font-variant-numeric: tabular-nums; padding: 3px 8px; border-radius: 999px; background: #fff; border-color: color-mix(in srgb, var(--ec) 45%, #fff); color: color-mix(in srgb, var(--ec) var(--ink, 78%), #000); box-shadow: var(--shadow-1); animation: pop-in var(--dur-base) var(--ease-spring) both; }
.edge-label.flow.up { --ec: var(--flow-up); }
.edge-label.flow.down { --ec: var(--flow-down); }
.edge-label.flow.lost { --ec: var(--flow-lost); background: var(--crit-bg); }
/* amber is the lightest of the tones: its text needs a darker ink to keep 4.5:1 on its tint */
.edge-label.flow.missed { --ec: var(--flow-missed); --ink: 64%; background: var(--risk-bg); }
.edge-break { position: absolute; pointer-events: none; width: 18px; height: 18px; border-radius: 50%; display: inline-flex; align-items: center; justify-content: center; background: var(--crit); color: #fff; box-shadow: 0 0 0 3px var(--crit-bg); z-index: 31; animation: pop-in var(--dur-base) var(--ease-spring) both; }
.vue-flow__edge-path { transition: stroke-width var(--dur-instant), stroke var(--dur-base), opacity var(--dur-base); animation: edge-in var(--dur-slow) var(--ease-out) both; animation-delay: var(--dur-base); }
.vue-flow__edge-path.edge-traffic { transition: stroke-width var(--dur-slower) var(--ease-out), stroke var(--dur-slow), opacity var(--dur-slow); }
@keyframes edge-in { from { opacity: 0; } }
.lost-band { stroke: var(--flow-lost); stroke-linecap: round; stroke-dasharray: 5 5; opacity: .85; pointer-events: none; }
.lost-band.soft { stroke: var(--flow-missed); }
.vue-flow__edge:hover .vue-flow__edge-path { stroke-width: 3; }
g.dim { opacity: .18; }
g.quiet { opacity: .55; }
g.faded { opacity: .14; }
/* a failure is on screen: the links nothing goes along step back, the ones that carry or lose something stand out */
g.traffic.idle { opacity: .3; transition: opacity var(--dur-slow); }
g.step-back { opacity: .42; transition: opacity var(--dur-slow); }
g.traffic.carrying, g.traffic.dropping, g.traffic.missing { opacity: 1; transition: opacity var(--dur-slow); }
</style>
