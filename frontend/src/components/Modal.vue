<script setup lang="ts">
import { onBeforeUnmount, onMounted } from 'vue'
import { X } from 'lucide-vue-next'
import { t } from '@/i18n'

const props = withDefaults(defineProps<{ title?: string; size?: 'md' | 'lg' | 'xl'; closable?: boolean }>(), { size: 'md', closable: true })
const emit = defineEmits<{ close: [] }>()

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape' && props.closable) { e.stopPropagation(); emit('close') }
}
onMounted(() => window.addEventListener('keydown', onKey, true))
onBeforeUnmount(() => window.removeEventListener('keydown', onKey, true))
</script>

<template>
  <Teleport to="body">
    <div class="modal-backdrop" @mousedown.self="closable && emit('close')">
      <div class="modal" :class="size === 'lg' ? 'modal-lg' : size === 'xl' ? 'modal-xl' : ''" role="dialog" aria-modal="true" :aria-label="title">
        <div class="modal-head">
          <h2 class="grow truncate"><slot name="title">{{ title }}</slot></h2>
          <slot name="head" />
          <button v-if="closable" class="btn btn-ghost btn-icon" :aria-label="t('common.close')" @click="emit('close')"><X :size="18" /></button>
        </div>
        <div class="modal-body"><slot /></div>
        <div v-if="$slots.footer" class="modal-foot"><slot name="footer" /></div>
      </div>
    </div>
  </Teleport>
</template>
