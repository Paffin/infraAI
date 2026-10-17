<script setup lang="ts">
import { ref, watchEffect } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'
import { t } from '@/i18n'
import { focusIsFree, focusPage } from '@/lib/pageFocus'
import { pageTitle } from '@/lib/pageTitle'
import ToastHost from '@/components/ToastHost.vue'
import DialogHost from '@/components/DialogHost.vue'
import TourHost from '@/components/TourHost.vue'

const app = useApp()
const editor = useEditor()
const route = useRoute()
const router = useRouter()

// ---- the title of the tab: ONE effect that follows the page, the project's name (it changes on rename), the language and the service's name
watchEffect(() => {
  const open = route.name === 'editor' && editor.project?.id === route.params.id ? editor.project.name : null
  document.title = pageTitle({ routeName: route.name, params: route.params, projectName: open, appName: app.appName })
})

// ---- a new page takes the keyboard: the focus goes to its heading, which a screen reader then reads (the page title is announced that way, with no
// second live region to say it again). Only when the page really changes (not the first one, not `?sim=`, not a move between the sections of the admin
// panel, where the sidebar link keeps the focus), and only when nothing in the new page has the focus already (the sign-in form has its field).
const focusFor = ref<string | null>(null)
const entered = ref(false)
router.afterEach((to, from) => {
  if (from.name === undefined || to.name === from.name) return
  focusFor.value = String(to.name)
  entered.value = false
})
// the page is not there before its transition ends: the router has long finished by then
const onPageEntered = () => { entered.value = true }
watchEffect(() => {
  if (focusFor.value === null || !entered.value) return
  if (focusFor.value !== String(route.name)) { focusFor.value = null; return }
  if (route.name === 'editor' && editor.loading) return // the heading of the editor is the project's name: it is there when the project is
  focusFor.value = null
  if (focusIsFree()) focusPage()
})
</script>

<template>
  <div v-if="!app.ready" class="boot"><span class="spinner" />{{ t('app.loading') }}</div>
  <div v-else-if="app.bootError" class="boot-error">
    <div class="banner banner-err">{{ t('error.generic', { message: app.bootError }) }}</div>
    <button class="btn" @click="app.init()">{{ t('common.retry') }}</button>
  </div>
  <router-view v-else v-slot="{ Component, route }">
    <Transition name="page" mode="out-in" @after-enter="onPageEntered">
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
