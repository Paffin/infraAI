<script setup lang="ts">
import { useToasts, type Toast } from '@/stores/toasts'
const toasts = useToasts()

function act(x: Toast) {
  const run = x.action?.run
  toasts.dismiss(x.id)
  run?.()
}
</script>

<template>
  <TransitionGroup name="toast" tag="div" class="toast-host" aria-live="polite">
    <div v-for="x in toasts.items" :key="x.id" class="toast" :class="[x.kind, x.action && 'has-action']" :style="{ '--toast-ms': x.ms }" @click="toasts.dismiss(x.id)">
      <span class="txt">{{ x.text }}</span>
      <button v-if="x.action" class="act" data-testid="toast-action" @click.stop="act(x)">{{ x.action.label }}</button>
      <i class="timer" aria-hidden="true" />
    </div>
  </TransitionGroup>
</template>
