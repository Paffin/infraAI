<script setup lang="ts">
import { useApp } from '@/stores/app'
import { t } from '@/i18n'
import ToastHost from '@/components/ToastHost.vue'
import DialogHost from '@/components/DialogHost.vue'
import TourHost from '@/components/TourHost.vue'

const app = useApp()
</script>

<template>
  <div v-if="!app.ready" class="boot"><span class="spinner" />{{ t('app.loading') }}</div>
  <div v-else-if="app.bootError" class="boot-error">
    <div class="banner banner-err">{{ t('error.generic', { message: app.bootError }) }}</div>
    <button class="btn" @click="app.init()">{{ t('common.retry') }}</button>
  </div>
  <router-view v-else v-slot="{ Component, route }">
    <Transition name="page" mode="out-in">
      <component :is="Component" :key="String(route.name)" />
    </Transition>
  </router-view>
  <ToastHost />
  <DialogHost />
  <TourHost v-if="app.ready && app.user" />
</template>

<style>
.boot { height: 100%; display: flex; gap: 10px; align-items: center; justify-content: center; color: var(--muted); }
.boot-error { max-width: 480px; margin: 20vh auto; display: flex; flex-direction: column; gap: 12px; align-items: flex-start; }
</style>
