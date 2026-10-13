<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { DURATIONS, tween, type Tween } from '@/lib/motion'
import { useMotion } from '@/stores/motion'

/**
 * A number that counts to its new value instead of jumping. The element always carries the final value
 * (`data-value`), so tests and screen readers never see the middle of the count. Without animations it just changes.
 */
const props = withDefaults(defineProps<{
  value: number | null | undefined
  /** how the number is written; the default rounds to `decimals` */
  format?: (n: number) => string
  decimals?: number
  /** the shown value when there is none */
  empty?: string
  ms?: number
}>(), { decimals: 0, empty: '—', ms: DURATIONS.slow })

const motion = useMotion()
const shown = ref(props.value ?? 0)
let running: Tween | null = null

watch(() => props.value, (to) => {
  running?.cancel()
  if (to === null || to === undefined || !Number.isFinite(to)) return
  if (!motion.full) { shown.value = to; return }
  running = tween(shown.value, to, props.ms * motion.factor, (v) => { shown.value = v })
})
onBeforeUnmount(() => running?.cancel())

const factor = computed(() => Math.pow(10, props.decimals))
const text = computed(() => {
  if (props.value === null || props.value === undefined || !Number.isFinite(props.value)) return props.empty
  const n = Math.round(shown.value * factor.value) / factor.value
  return props.format ? props.format(n) : n.toFixed(props.decimals)
})
</script>

<template>
  <span class="anum" :data-value="value ?? ''">{{ text }}</span>
</template>

<style scoped>
.anum { font-variant-numeric: tabular-nums; }
</style>
