<script setup lang="ts">
import { ref } from 'vue'
import { Check, Copy } from 'lucide-vue-next'
import { t } from '@/i18n'

const props = defineProps<{ text: string; label?: string; small?: boolean }>()
const done = ref(false)

async function copy() {
  try {
    await navigator.clipboard.writeText(props.text)
  } catch {
    // clipboard API unavailable (insecure context): fall back to a temporary textarea
    const el = document.createElement('textarea')
    el.value = props.text
    el.style.position = 'fixed'
    el.style.opacity = '0'
    document.body.appendChild(el)
    el.select()
    try { document.execCommand('copy') } catch { /* ignore */ }
    el.remove()
  }
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
