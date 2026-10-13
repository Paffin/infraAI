<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import type { Component } from 'vue'

/** An icon button in a dark header that opens a small menu under it; clicking outside or Esc closes it. */
defineProps<{ icon: Component; label: string; width?: number }>()

const open = ref(false)
const root = ref<HTMLElement | null>(null)

function close() { open.value = false }
function onDown(e: MouseEvent) {
  if (open.value && root.value && !root.value.contains(e.target as Node)) open.value = false
}
function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape' && open.value) { open.value = false; e.stopPropagation() }
}
onMounted(() => { document.addEventListener('mousedown', onDown); document.addEventListener('keydown', onKey, true) })
onBeforeUnmount(() => { document.removeEventListener('mousedown', onDown); document.removeEventListener('keydown', onKey, true) })
</script>

<template>
  <div ref="root" class="hmenu">
    <button class="trigger" :class="{ on: open }" :aria-expanded="open" aria-haspopup="menu" :title="label" :aria-label="label" @click="open = !open">
      <component :is="icon" :size="15" />
    </button>
    <Transition name="pop">
      <div v-if="open" class="menu" role="menu" :aria-label="label" :style="{ width: (width ?? 272) + 'px' }">
        <div class="head">{{ label }}</div>
        <slot :close="close" />
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.hmenu { position: relative; display: inline-flex; }
.trigger { width: 30px; height: 28px; display: inline-flex; align-items: center; justify-content: center; border: 1px solid var(--line-on-dark); border-radius: var(--radius-sm); background: transparent; color: var(--on-dark-muted); }
.trigger:hover, .trigger.on { color: #fff; background: var(--ink-3); }
.menu { position: absolute; right: 0; top: calc(100% + 8px); z-index: 120; background: var(--surface); color: var(--text); border: 1px solid var(--line); border-radius: var(--radius); box-shadow: var(--shadow-3); padding: 6px; }
.head { padding: 6px 10px 4px; font-size: 11px; font-weight: 700; letter-spacing: .06em; text-transform: uppercase; color: var(--muted); }
</style>
