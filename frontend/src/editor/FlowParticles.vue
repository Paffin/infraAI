<script setup lang="ts">
// The requests on a link, as particles that run along it (SVG motion along the link's own path). What the link carries runs from end to end;
// what is dropped on it runs to the place where it is dropped and fades there: red where the requests behind it fail, amber where only a call to
// an optional dependency goes unanswered. Particles are decoration of a number the engine computed: their density is the flow, their count is
// capped for a big diagram (`editor.flowParticlesMax`), and they run only with full animation.
import { computed } from 'vue'
import { useVueFlow } from '@vue-flow/core'
import { dropAt, particleCount } from '@/lib/traffic'
import { useApp } from '@/stores/app'
import { useMotion } from '@/stores/motion'
import { usePlayback } from '@/stores/playback'

const props = withDefaults(defineProps<{
  /** the path of the link */
  d: string
  /** its length in px (the engine does not know it: the distance between the ends is close enough) */
  length: number
  flow: number
  /** dropped on the link, the requests behind it fail */
  lost?: number
  /** dropped on the link, the requests survive */
  missed?: number
  /** the link is cut: what is dropped on it is dropped in the middle, not at the card */
  cut?: boolean
  color?: string
}>(), { lost: 0, missed: 0, cut: false, color: 'var(--flow)' })

const app = useApp()
const motion = useMotion()
const playback = usePlayback()
const { viewport } = useVueFlow()

/** The n-th offset of a sequence that stays evenly spread however many of its first terms are taken: a particle that appears does not move the others. */
const spread = (k: number) => (k * 0.6180339887) % 1

const on = computed(() => motion.full && playback.particleScale > 0)
const count = (flow: number, len: number) => (on.value ? particleCount(flow, playback.flowRef, len, app.editorCfg.flowSpacing, playback.particleScale) : 0)
const carried = computed(() => count(props.flow, props.length))
const drop = computed(() => dropAt(props.cut))
const lostN = computed(() => count(props.lost, props.length * drop.value))
const missedN = computed(() => count(props.missed, props.length * drop.value))
const secs = computed(() => Math.max(0.8, props.length / Math.max(1, app.editorCfg.flowSpeed)))
const dropSecs = computed(() => Math.max(0.6, (props.length * drop.value) / Math.max(1, app.editorCfg.flowSpeed)))
const begin = (k: number, dur: number) => `${(-spread(k) * dur).toFixed(3)}s`

/** A particle keeps a readable size on the screen however far the diagram is zoomed out (the zoom is rounded, so that it does not repaint every one of them on every step of the wheel). */
const radius = computed(() => {
  const z = Math.max(0.15, Math.round((viewport.value.zoom || 1) * 10) / 10)
  return Math.min(7, Math.max(2.7, 2.1 / z))
})
</script>

<template>
  <g v-if="carried || lostN || missedN" class="particles" aria-hidden="true">
    <circle v-for="k in carried" :key="'c' + k" :r="radius" class="pc" :style="{ fill: color }">
      <animateMotion :path="d" :dur="secs + 's'" :begin="begin(k, secs)" repeatCount="indefinite" calcMode="linear" />
    </circle>
    <template v-for="(group, name) in { pl: lostN, pm: missedN }" :key="name">
      <circle v-for="k in group" :key="name + k" :r="radius + 0.2" :class="name">
        <animateMotion :path="d" :dur="dropSecs + 's'" :begin="begin(k + 5, dropSecs)" repeatCount="indefinite" calcMode="linear" keyPoints="0;1" keyTimes="0;1" />
        <animate attributeName="opacity" values="1;1;0" keyTimes="0;0.72;1" :dur="dropSecs + 's'" :begin="begin(k + 5, dropSecs)" repeatCount="indefinite" />
        <animate attributeName="r" :values="`${radius};${radius};${radius * 1.9}`" keyTimes="0;0.72;1" :dur="dropSecs + 's'" :begin="begin(k + 5, dropSecs)" repeatCount="indefinite" />
      </circle>
    </template>
  </g>
</template>

<style>
.particles .pc { opacity: .95; }
.particles .pl { fill: var(--flow-lost); }
.particles .pm { fill: var(--flow-missed); }
</style>
