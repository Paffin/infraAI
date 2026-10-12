<script setup lang="ts">
import { useToasts } from '@/stores/toasts'
const toasts = useToasts()
</script>

<template>
  <div class="toast-host" aria-live="polite">
    <div v-for="x in toasts.items" :key="x.id" class="toast" :class="x.kind" @click="toasts.dismiss(x.id)">{{ x.text }}</div>
  </div>
</template>
