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
  <!-- In the body, beside the page and not inside it: a dialog makes the page inert, and a message that a dialog raises ("the version was not restored")
       has to stay readable and clickable over it. `data-keep-live` keeps it out of what a dialog locks (lib/inert.ts). Pressing a message does not take the
       keyboard from the dialog (or the editor) it came from: `@mousedown.prevent`. -->
  <Teleport to="body">
    <TransitionGroup name="toast" tag="div" class="toast-host" aria-live="polite" data-keep-live>
      <div v-for="x in toasts.items" :key="x.id" class="toast" :class="[x.kind, x.action && 'has-action']" :style="{ '--toast-ms': x.ms }" @mousedown.prevent @click="toasts.dismiss(x.id)">
        <span class="txt">{{ x.text }}</span>
        <button v-if="x.action" class="act" data-testid="toast-action" @click.stop="act(x)">{{ x.action.label }}</button>
        <i class="timer" aria-hidden="true" />
      </div>
    </TransitionGroup>
  </Teleport>
</template>
