<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { t } from '@/i18n'
import { fmtAxis, fmtUsd, niceMax, type UsageDay } from '../lib/llm'

const props = defineProps<{ days: UsageDay[] }>()

const box = ref<HTMLElement | null>(null)
const width = ref(720)
const height = 190
const pad = { l: 46, r: 8, t: 10, b: 24 }
const hover = ref(-1)
let ro: ResizeObserver | undefined

onMounted(() => {
  if (!box.value) return
  width.value = box.value.clientWidth || 720
  if (typeof ResizeObserver !== 'undefined') {
    ro = new ResizeObserver(() => { width.value = box.value?.clientWidth || width.value })
    ro.observe(box.value)
  }
})
onBeforeUnmount(() => ro?.disconnect())

const max = computed(() => niceMax(Math.max(0, ...props.days.map((d) => d.costUsd))))
const plotW = computed(() => Math.max(40, width.value - pad.l - pad.r))
const plotH = height - pad.t - pad.b
const band = computed(() => plotW.value / Math.max(1, props.days.length))
const barW = computed(() => Math.max(2, Math.min(22, band.value * 0.62)))
const ticks = computed(() => [0, 0.5, 1].map((f) => ({ y: pad.t + plotH - f * plotH, label: fmtAxis(max.value * f, max.value) })))

const bars = computed(() => props.days.map((d, i) => {
  const h = max.value ? (d.costUsd / max.value) * plotH : 0
  return { i, x: pad.l + i * band.value + (band.value - barW.value) / 2, h, y: pad.t + plotH - h, d }
}))

/** one label per ~90px so they never collide */
const labelEvery = computed(() => Math.max(1, Math.ceil(90 / band.value)))
function dayLabel(day: string): string { return day.slice(5) }

const tip = computed(() => {
  const b = bars.value[hover.value]
  if (!b) return null
  const left = Math.min(width.value - 170, Math.max(0, b.x + barW.value / 2 - 80))
  return { left, top: Math.max(0, b.y - 74), d: b.d }
})
</script>

<template>
  <div ref="box" class="chart" @mouseleave="hover = -1">
    <svg :width="width" :height="height" role="img" :aria-label="t('admin.llm.chartLabel')">
      <g class="chart-grid">
        <g v-for="tk in ticks" :key="tk.y">
          <line :x1="pad.l" :x2="width - pad.r" :y1="tk.y" :y2="tk.y" />
          <text :x="pad.l - 6" :y="tk.y + 4" text-anchor="end">{{ tk.label }}</text>
        </g>
      </g>
      <g>
        <template v-for="b in bars" :key="b.d.day">
          <path v-if="b.h > 0" class="chart-bar" :class="{ on: hover === b.i }" :d="`M${b.x},${pad.t + plotH} V${b.y + Math.min(4, b.h)} Q${b.x},${b.y} ${b.x + Math.min(4, barW / 2)},${b.y} H${b.x + barW - Math.min(4, barW / 2)} Q${b.x + barW},${b.y} ${b.x + barW},${b.y + Math.min(4, b.h)} V${pad.t + plotH} Z`" />
          <rect v-else class="chart-zero" :x="b.x" :y="pad.t + plotH - 1" :width="barW" height="1" />
          <rect class="chart-hit" :x="pad.l + b.i * band" :y="pad.t" :width="band" :height="plotH" @mouseenter="hover = b.i" />
        </template>
      </g>
      <g class="chart-axis">
        <text v-for="b in bars.filter((x) => (bars.length - 1 - x.i) % labelEvery === 0)" :key="b.d.day" :x="b.x + barW / 2" :y="height - 6" text-anchor="middle">{{ dayLabel(b.d.day) }}</text>
      </g>
    </svg>
    <div v-if="tip" class="chart-tip" :style="{ left: tip.left + 'px', top: tip.top + 'px' }" role="status">
      <strong>{{ tip.d.day }}</strong>
      <span>{{ fmtUsd(tip.d.costUsd) }}</span>
      <span class="faint">{{ t('admin.llm.tipCalls', { n: tip.d.calls, errors: tip.d.errors }) }}</span>
    </div>
  </div>
</template>
