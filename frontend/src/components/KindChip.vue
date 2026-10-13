<script setup lang="ts">
// The face of a kind of component: its icon on a tint of its colour (or, solid, its abbreviation on its colour: where an icon would not read, a
// diagram zoomed far out). One look for the card on the canvas, the tile in the library and the line in the command palette.
import { computed } from 'vue'
import type { KindDef } from '@/api/types'
import { iconFor } from '@/lib/icons'
import { useApp } from '@/stores/app'

const props = withDefaults(defineProps<{
  kind?: KindDef | null
  /** edge of the square, px */
  size?: number
  solid?: boolean
}>(), { kind: null, size: 38, solid: false })

const app = useApp()
const icon = computed(() => iconFor(props.kind?.icon, app.index?.categories.get(props.kind?.category ?? '')?.icon))
const color = computed(() => props.kind?.color ?? '#4c596c')
</script>

<template>
  <span class="kchip" :class="{ solid }" :style="{ '--kc': color, width: size + 'px', height: size + 'px', borderRadius: Math.round(size * 0.27) + 'px' }" aria-hidden="true">
    <component :is="icon" class="ic" :size="Math.round(size * 0.52)" :stroke-width="1.9" />
    <b class="ab">{{ kind?.abbr ?? '?' }}</b>
  </span>
</template>

<style scoped>
.kchip { --kc: #4c596c; position: relative; flex: none; display: inline-flex; align-items: center; justify-content: center; color: color-mix(in srgb, var(--kc) 80%, #000); background: linear-gradient(180deg, color-mix(in srgb, var(--kc) 8%, #fff), color-mix(in srgb, var(--kc) 17%, #fff)); box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--kc) 24%, #fff); }
.ab { display: none; font-size: 10px; font-weight: 700; letter-spacing: .02em; }
.kchip.solid { background: var(--kc); color: #fff; box-shadow: none; }
.kchip.solid .ic { display: none; }
.kchip.solid .ab { display: inline; }
</style>
