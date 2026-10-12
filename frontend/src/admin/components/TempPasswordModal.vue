<script setup lang="ts">
import { TriangleAlert } from 'lucide-vue-next'
import Modal from '@/components/Modal.vue'
import { t } from '@/i18n'
import CopyButton from './CopyButton.vue'

defineProps<{ email: string; password: string; created: boolean }>()
defineEmits<{ close: [] }>()
</script>

<template>
  <Modal :title="created ? t('admin.users.tempCreated') : t('admin.users.tempReset')" :closable="false">
    <div class="col" style="gap: 12px">
      <p style="margin: 0">{{ t('admin.users.tempFor', { email }) }}</p>
      <div class="temp-pass">
        <code class="mono" data-testid="temp-password">{{ password }}</code>
        <CopyButton :text="password" :label="t('admin.users.copyPassword')" />
      </div>
      <div class="banner banner-warn"><TriangleAlert :size="14" /> {{ t('admin.users.tempWarn') }}</div>
      <p class="muted" style="margin: 0">{{ t('admin.users.tempNote') }}</p>
    </div>
    <template #footer>
      <button class="btn btn-primary" @click="$emit('close')">{{ t('admin.users.tempDone') }}</button>
    </template>
  </Modal>
</template>
