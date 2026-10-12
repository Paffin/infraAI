<script setup lang="ts">
defineProps<{ size?: number }>()
</script>

<template>
  <svg :width="size ?? 28" :height="size ?? 28" viewBox="0 0 28 28" aria-hidden="true">
    <rect width="28" height="28" rx="7" fill="var(--accent)" />
    <g fill="none" stroke="#fff" stroke-width="1.7" stroke-linejoin="round">
      <rect x="6.5" y="6.5" width="5.5" height="5.5" rx="1.6" />
      <rect x="16" y="6.5" width="5.5" height="5.5" rx="1.6" />
      <rect x="11.25" y="16" width="5.5" height="5.5" rx="1.6" />
    </g>
  </svg>
</template>
