<script setup lang="ts">
import { Check, CircleAlert } from 'lucide-vue-next'
import { t } from '@/i18n'

defineProps<{ count: number; saving?: boolean; status?: { kind: 'ok' | 'error'; text: string } | null }>()
defineEmits<{ save: []; discard: [] }>()
</script>

<template>
  <div class="savebar-wrap">
    <span class="sr-only" role="status" aria-live="polite">{{ status?.text }}</span>
    <div v-if="count > 0 || status?.kind === 'ok'" class="savebar">
      <span class="savebar-msg">
        <template v-if="count > 0">
          <span class="dot" />{{ t('admin.settings.pending', { n: count }) }}
          <span v-if="status?.kind === 'error'" class="savebar-err"><CircleAlert :size="14" />{{ status.text }}</span>
        </template>
        <span v-else class="savebar-ok"><Check :size="14" />{{ status?.text }}</span>
      </span>
      <span class="spacer" />
      <template v-if="count > 0">
        <button type="button" class="btn" :disabled="saving" @click="$emit('discard')">{{ t('admin.settings.discard') }}</button>
        <button type="button" class="btn btn-primary" :disabled="saving" @click="$emit('save')"><span v-if="saving" class="spinner" />{{ t('common.save') }}</button>
      </template>
    </div>
  </div>
</template>
