<script setup lang="ts">
import { ref } from 'vue'
import { Check, Copy } from 'lucide-vue-next'
import { t } from '@/i18n'
import { copyText } from '@/lib/clipboard'

const props = defineProps<{ text: string; label?: string; small?: boolean }>()
const done = ref(false)

async function copy() {
  await copyText(props.text)
  done.value = true
  setTimeout(() => { done.value = false }, 1800)
}
</script>

<template>
  <button type="button" class="btn" :class="{ 'btn-sm': small }" @click="copy">
    <Check v-if="done" :size="14" /><Copy v-else :size="14" />
    <span aria-live="polite">{{ done ? t('admin.copied') : (label ?? t('common.copy')) }}</span>
  </button>
</template>
