<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, shallowRef, watch } from 'vue'
import * as api from '@/api/endpoints'
import type { Architecture, ProjectThumb } from '@/api/types'
import { placeMissing } from '@/editor/layout'
import { buildScene, EMPTY_SCENE, themePalette, thumbToArchitecture } from '@/lib/diagram'
import { useApp } from '@/stores/app'

const props = defineProps<{ id: string; thumb: ProjectThumb | null; nodeCount: number }>()
const app = useApp()
const root = ref<HTMLElement | null>(null)
/** Cards far below the fold are drawn when they scroll into view: laying out a hundred diagrams at once is wasted work. */
const seen = ref(typeof IntersectionObserver === 'undefined')
let observer: IntersectionObserver | undefined

onMounted(() => {
  if (seen.value || !root.value) return
  observer = new IntersectionObserver((entries) => {
    if (entries.some((e) => e.isIntersecting)) { seen.value = true; observer?.disconnect() }
  }, { rootMargin: '200px' })
  observer.observe(root.value)
})
onBeforeUnmount(() => observer?.disconnect())

// The list brings the thumbnail of the most recently changed projects only (`limits.listThumbnails`); for the rest the
// project is loaded when its card comes into view – a few at a time, so that scrolling a long list does not flood the server.
const MAX_PARALLEL = 3
let running = 0
const waiting: (() => void)[] = []
async function limited<T>(job: () => Promise<T>): Promise<T> {
  if (running >= MAX_PARALLEL) await new Promise<void>((resolve) => waiting.push(resolve))
  running++
  try { return await job() } finally { running--; waiting.shift()?.() }
}

const loaded = shallowRef<Architecture | null>(null)
let requested = false
watch(seen, async (v) => {
  if (!v || props.thumb || props.nodeCount === 0 || requested) return
  requested = true
  try { loaded.value = (await limited(() => api.projects.get(props.id))).architecture } catch { /* the placeholder stays */ }
}, { immediate: true })

const palette = themePalette()
const scene = computed(() => {
  const ix = app.index
  if (!seen.value || !ix) return EMPTY_SCENE
  if (props.thumb) return props.thumb.nodes.length ? buildScene(thumbToArchitecture(props.thumb, ix), ix, { detail: 'thumb', palette }) : EMPTY_SCENE
  const a = loaded.value
  return a && a.nodes.length ? buildScene(placeMissing(a, ix), ix, { detail: 'thumb', palette }) : EMPTY_SCENE
})
const empty = computed(() => !scene.value.shapes.length)
</script>

<template>
  <div ref="root" class="thumb" :class="{ empty }">
    <svg v-if="!empty" :viewBox="`${scene.x} ${scene.y} ${scene.w} ${scene.h}`" preserveAspectRatio="xMidYMid meet" aria-hidden="true">
      <template v-for="(s, i) in scene.shapes" :key="i">
        <rect v-if="s.t === 'rect'" :x="s.x" :y="s.y" :width="s.w" :height="s.h" :rx="s.rx" :fill="s.fill" :stroke="s.stroke" :stroke-width="s.sw" :opacity="s.opacity" />
        <path v-else-if="s.t === 'path'" :d="s.d" fill="none" :stroke="s.stroke" :stroke-width="s.sw" stroke-linecap="round" :stroke-dasharray="s.dash" :opacity="s.opacity" />
        <polygon v-else-if="s.t === 'poly'" :points="s.points" :fill="s.fill" :opacity="s.opacity" />
        <circle v-else-if="s.t === 'circle'" :cx="s.cx" :cy="s.cy" :r="s.r" :fill="s.fill" :stroke="s.stroke" :stroke-width="s.sw" />
      </template>
    </svg>
  </div>
</template>

<style scoped>
.thumb { height: 128px; background: var(--surface-2); border-bottom: 1px solid var(--line); display: flex; align-items: center; justify-content: center; padding: 8px 10px; }
.thumb svg { width: 100%; height: 100%; display: block; }
.thumb.empty::before { content: ''; width: 46px; height: 30px; border: 1.5px dashed var(--line-strong); border-radius: 6px; }
</style>
