<script setup lang="ts">
import { computed, ref } from 'vue'
import { BaseEdge, EdgeLabelRenderer, getBezierPath, getStraightPath, Position, type EdgeProps } from '@vue-flow/core'
import { lt, t } from '@/i18n'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'
import { usePlayback } from '@/stores/playback'

const props = defineProps<EdgeProps>()
const app = useApp()
const editor = useEditor()
const playback = usePlayback()

const id = computed(() => props.data.id as string)
const edge = computed(() => editor.edgeById.get(id.value))
const def = computed(() => (edge.value ? app.index?.edgeTypes.get(edge.value.type) : undefined))
const selected = computed(() => editor.selection?.type === 'edge' && editor.selection.id === id.value)
const simState = computed(() => (editor.sim?.result ? playback.edgeStates[id.value] : undefined))
/** While a failure is played back, the requests visibly keep flowing along the edges that still carry them. */
const flowing = computed(() => playback.playing && simState.value === 'ok' && def.value?.semantics === 'dependency')
const status = computed(() => editor.edgeStatus.get(id.value))
const added = computed(() => !!editor.previewSets?.added.edges.has(id.value))
const ghost = computed(() => !!editor.previewSets?.removed.edges.has(id.value))
const lit = computed(() => !!editor.highlight?.edges.includes(id.value))

const color = computed(() => (simState.value === 'cut' ? 'var(--crit)' : simState.value === 'dead' ? 'var(--faint)' : def.value?.style.color ?? 'var(--edge-traffic)'))
const width = computed(() => (def.value?.style.width ?? 1.6) + (selected.value || lit.value ? 1.4 : 0))
const dash = computed(() => (simState.value === 'cut' ? '5 4' : ghost.value ? '4 4' : def.value?.style.dash ?? undefined))
const dimmed = computed(() => {
  if (editor.highlight && !lit.value) return true
  if (editor.layer !== 'all' && (def.value?.layer ?? 'network') !== editor.layer) return true
  return simState.value === 'dead'
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
const showLabel = computed(() => !!label.value && (selected.value || hover.value || lit.value || (long.value && !quiet.value)))

const markerId = computed(() => `mk-${props.id}`)
const arrow = computed(() => def.value?.directed !== false)
</script>

<template>
  <g v-if="edge" :class="{ dim: dimmed, quiet: quiet && !selected && !hover }" @mouseenter="hover = true" @mouseleave="hover = false">
    <defs v-if="arrow">
      <marker :id="markerId" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="8" markerHeight="8" orient="auto-start-reverse" markerUnits="userSpaceOnUse">
        <path d="M0,1 L9,5 L0,9 Z" :fill="color" />
      </marker>
    </defs>
    <BaseEdge
      :id="id" :path="path.d" :marker-end="arrow ? `url(#${markerId})` : undefined" :interaction-width="20"
      :style="{ stroke: color, strokeWidth: width, strokeDasharray: dash, opacity: ghost ? 0.5 : 1 }"
      :class="{ 'edge-added': added, 'edge-risk': status === 'risk' || status === 'critical', 'edge-flow': flowing }"
    />
  </g>
  <EdgeLabelRenderer v-if="edge && showLabel">
    <div
      class="edge-label nodrag nopan" :class="{ selected, dim: dimmed, added }" @mouseenter="hover = true" @mouseleave="hover = false"
      :style="{ transform: `translate(-50%, -50%) translate(${path.lx}px, ${path.ly}px)`, '--ec': color }"
      :title="def ? lt(def.name) : edge.type"
      @click.stop="editor.select({ type: 'edge', id })"
    >{{ label }}</div>
  </EdgeLabelRenderer>
</template>

<style>
.edge-label {
  position: absolute; pointer-events: all; cursor: pointer; font-family: var(--mono); font-size: 11px; line-height: 1; padding: 4px 8px; border-radius: 5px;
  background: color-mix(in srgb, var(--ec) 7%, #fff); border: 1px solid color-mix(in srgb, var(--ec) 55%, #fff); color: color-mix(in srgb, var(--ec) 60%, #000); white-space: nowrap; z-index: 30;
}
.edge-label.selected { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
.edge-label.dim { opacity: .25; }
.edge-label.added { border-style: dashed; }
.vue-flow__edge-path { transition: stroke-width var(--dur-instant), stroke var(--dur-base), opacity var(--dur-base); animation: edge-in var(--dur-slow) var(--ease-out) both; animation-delay: var(--dur-base); }
@keyframes edge-in { from { opacity: 0; } }
.vue-flow__edge-path.edge-flow { stroke-dasharray: 6 8 !important; animation: edge-flow .9s linear infinite; }
@keyframes edge-flow { to { stroke-dashoffset: -28; } }
.vue-flow__edge:hover .vue-flow__edge-path { stroke-width: 3; }
g.dim { opacity: .18; }
g.quiet { opacity: .55; }
</style>
