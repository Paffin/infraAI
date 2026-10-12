<script setup lang="ts">
import { useApp } from '@/stores/app'
import { t } from '@/i18n'
import ToastHost from '@/components/ToastHost.vue'
import DialogHost from '@/components/DialogHost.vue'

const app = useApp()
</script>

<template>
  <div v-if="app.bootError" class="boot-error">
    <div class="banner banner-err">{{ t('error.generic', { message: app.bootError }) }}</div>
    <button class="btn" @click="app.init()">{{ t('common.retry') }}</button>
  </div>
  <router-view v-else />
  <ToastHost />
  <DialogHost />
</template>

<style>
.boot-error { max-width: 480px; margin: 20vh auto; display: flex; flex-direction: column; gap: 12px; align-items: flex-start; }
</style>
