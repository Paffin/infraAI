<script setup lang="ts">
// A route that no drawn link leads along: the traffic of one node goes to another because every link it has to that unit is gone (the unit
// model lets a member reach any member of the unit it depends on that serves). The engine says so in `Traffic.detours`; the canvas has an edge for
// each detour of the story and this component draws it in the pictures that have it.
import { computed } from 'vue'
import { EdgeLabelRenderer, getBezierPath, Position, type EdgeProps } from '@vue-flow/core'
import { t } from '@/i18n'
import { fmtNumber } from '@/lib/format'
import { amountText, flowWidth, FLOW_EPS } from '@/lib/traffic'
import { useEditor } from '@/stores/editor'
import { usePlayback } from '@/stores/playback'
import FlowParticles from './FlowParticles.vue'

const props = defineProps<EdgeProps>()
const editor = useEditor()
const playback = usePlayback()

const from = computed(() => props.data.from as string)
const to = computed(() => props.data.to as string)
const detour = computed(() => playback.traffic?.detours?.find((d) => d.from === from.value && d.to === to.value) ?? null)
const flow = computed(() => detour.value?.flow ?? 0)
const lost = computed(() => detour.value?.lost ?? 0)
const shown = computed(() => !!editor.sim?.result && (flow.value > FLOW_EPS || lost.value > FLOW_EPS))

const path = computed(() => {
  const [p, lx, ly] = getBezierPath({ sourceX: props.sourceX, sourceY: props.sourceY, sourcePosition: props.sourcePosition ?? Position.Bottom, targetX: props.targetX, targetY: props.targetY, targetPosition: props.targetPosition ?? Position.Top, curvature: 0.5 })
  return { d: p, lx, ly }
})
const length = computed(() => Math.hypot(props.targetX - props.sourceX, props.targetY - props.sourceY) * 1.15 + 16)
const width = computed(() => flowWidth(flow.value || lost.value, playback.flowRef, 1.3, 3.2))
const text = computed(() => {
  const a = amountText(flow.value + lost.value, editor.arch.requirements.rpsPeak, fmtNumber)
  return t('traffic.detour.label', { amount: a.unit === 'rps' ? `${a.value} ${t('traffic.unit.rps')}` : `${a.value}%` })
})
const markerId = computed(() => `mk-${props.id}`)
</script>

<template>
  <Transition name="detour">
    <g v-if="shown" class="detour" aria-hidden="true">
      <defs>
        <marker :id="markerId" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="8" markerHeight="8" orient="auto-start-reverse" markerUnits="userSpaceOnUse"><path d="M0,1 L9,5 L0,9 Z" fill="var(--flow-detour)" /></marker>
      </defs>
      <path :d="path.d" class="detour-band" fill="none" :style="{ strokeWidth: width }" :marker-end="`url(#${markerId})`" />
      <FlowParticles :d="path.d" :length="length" :flow="flow" :lost="lost" color="var(--flow-detour)" />
    </g>
  </Transition>
  <EdgeLabelRenderer v-if="shown">
    <div class="edge-label flow detour-label nodrag nopan" :style="{ transform: `translate(-50%, -50%) translate(${path.lx}px, ${path.ly}px)` }">{{ text }}</div>
  </EdgeLabelRenderer>
</template>

<style>
.detour-band { stroke: var(--flow-detour-band); stroke-linecap: round; stroke-dasharray: 8 6; transition: stroke-width var(--dur-slower) var(--ease-out); }
.detour-enter-active, .detour-leave-active { transition: opacity var(--dur-slow) var(--ease-out); }
.detour-enter-from, .detour-leave-to { opacity: 0; }
.edge-label.flow.detour-label { --ec: var(--flow-detour); }
</style>
