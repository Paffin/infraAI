<script setup lang="ts">
import { LogOut, Settings, FolderKanban } from 'lucide-vue-next'
import { useRouter } from 'vue-router'
import { t } from '@/i18n'
import { useApp } from '@/stores/app'
import BrandLogo from './BrandLogo.vue'
import LangSwitch from './LangSwitch.vue'

const app = useApp()
const router = useRouter()

async function logout() {
  await app.logout()
  router.push({ name: 'login' })
}
</script>

<template>
  <header class="topbar">
    <router-link to="/" class="brand"><BrandLogo /><span>{{ app.appName }}</span></router-link>
    <nav class="nav">
      <router-link to="/" class="navlink" active-class="" exact-active-class="on"><FolderKanban :size="15" />{{ t('nav.projects') }}</router-link>
      <router-link v-if="app.isAdmin" to="/admin" class="navlink" active-class="on"><Settings :size="15" />{{ t('nav.admin') }}</router-link>
    </nav>
    <slot />
    <span class="spacer" />
    <LangSwitch />
    <div v-if="app.user" class="user" :title="app.user.email">
      <span class="avatar">{{ (app.user.name || app.user.email).slice(0, 1).toUpperCase() }}</span>
      <span class="uname truncate">{{ app.user.name || app.user.email }}</span>
    </div>
    <button class="btn btn-dark btn-sm" @click="logout"><LogOut :size="14" />{{ t('nav.logout') }}</button>
  </header>
</template>

<style scoped>
.topbar { height: var(--header-h); background: var(--ink); color: var(--on-dark); display: flex; align-items: center; gap: 14px; padding: 0 20px; flex: none; }
.brand { display: flex; align-items: center; gap: 12px; color: #fff; font-weight: 650; font-size: 17px; letter-spacing: -0.01em; }
.brand:hover { text-decoration: none; }
.nav { display: flex; gap: 4px; margin-left: 14px; }
.navlink { display: inline-flex; align-items: center; gap: 7px; height: 32px; padding: 0 12px; border-radius: var(--radius-sm); color: var(--on-dark-muted); font-weight: 500; font-size: 13.5px; }
.navlink:hover { color: #fff; text-decoration: none; background: var(--ink-2); }
.navlink.on { color: #fff; background: var(--ink-3); }
.user { display: flex; align-items: center; gap: 8px; max-width: 220px; color: var(--on-dark); font-size: 13px; }
.avatar { width: 26px; height: 26px; border-radius: 50%; background: var(--ink-3); display: inline-flex; align-items: center; justify-content: center; font-weight: 650; font-size: 12px; }
@media (max-width: 760px) { .uname { display: none; } }
</style>
