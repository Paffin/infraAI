<script setup lang="ts">
import { t } from '@/i18n'

defineProps<{ loading?: boolean; error?: string | null; empty?: boolean; emptyText?: string }>()
defineEmits<{ retry: [] }>()
</script>

<template>
  <div v-if="loading" class="adm-state" role="status"><span class="spinner" />{{ t('admin.loading') }}</div>
  <div v-else-if="error" class="adm-state adm-state-err">
    <div class="banner banner-err" role="alert">{{ error }}</div>
    <button class="btn btn-sm" @click="$emit('retry')">{{ t('common.retry') }}</button>
  </div>
  <div v-else-if="empty" class="empty">{{ emptyText }}</div>
  <slot v-else />
</template>
