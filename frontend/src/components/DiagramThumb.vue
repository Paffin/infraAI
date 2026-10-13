<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import type { ProjectThumb } from '@/api/types'
import { buildScene, EMPTY_SCENE, themePalette, thumbToArchitecture } from '@/lib/diagram'
import { useApp } from '@/stores/app'

const props = defineProps<{ thumb: ProjectThumb | null }>()
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

const palette = themePalette()
const scene = computed(() => {
  const ix = app.index
  const t = props.thumb
  if (!seen.value || !ix || !t || !t.nodes.length) return EMPTY_SCENE
  return buildScene(thumbToArchitecture(t, ix), ix, { detail: 'thumb', palette })
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
