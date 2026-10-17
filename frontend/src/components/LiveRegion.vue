<script setup lang="ts">
import { liveMessage } from '@/lib/announce'
</script>

<template>
  <!-- The one place the editor speaks to a screen reader (lib/announce.ts): in the body, beside the page and not inside it. A dialog makes the page inert, and what the
       editor says while one is open (the verdict changed under an export) must still be read; `data-keep-live` keeps this out of what a dialog locks (lib/inert.ts), the
       way it keeps the toasts out. A live region has to be in the page BEFORE its text changes, so it is always drawn. -->
  <Teleport to="body">
    <div class="sr-only" role="status" aria-live="polite" aria-atomic="true" data-keep-live data-testid="live-region">{{ liveMessage }}</div>
  </Teleport>
</template>
