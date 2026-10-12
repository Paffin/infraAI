<script setup lang="ts">
import { computed, onMounted, watch } from 'vue'
import { onBeforeRouteLeave, onBeforeRouteUpdate, useRoute, useRouter } from 'vue-router'
import TopBar from '@/components/TopBar.vue'
import './admin.css'
import { confirmDiscard } from './lib/dirty'
import { guardKey } from './lib/nav'
import { CONFIG_SECTIONS, normalizeSection, SECTION_IDS } from './lib/configKinds'
import { useAdminConfig } from './store'
import SubNav from './components/SubNav.vue'
import AuditSection from './sections/AuditSection.vue'
import BackupSection from './sections/BackupSection.vue'
import ConfigSection from './sections/ConfigSection.vue'
import LlmSection from './sections/LlmSection.vue'
import SettingsSection from './sections/SettingsSection.vue'
import SystemSection from './sections/SystemSection.vue'
import UsersSection from './sections/UsersSection.vue'

const route = useRoute()
const router = useRouter()
const store = useAdminConfig()

const section = computed(() => normalizeSection(route.params.section))
const isConfig = computed(() => (CONFIG_SECTIONS as readonly string[]).includes(section.value))

// Canonical URLs: /admin and unknown sections go to /admin/settings.
function canonical() {
  const s = route.params.section
  if (!s || !(SECTION_IDS as readonly string[]).includes(String(s))) router.replace({ path: '/admin/settings', query: route.query })
}
watch(() => route.params.section, canonical)

onMounted(() => {
  canonical()
  store.load()
})

// Unsaved edits: ask before leaving the section (or the admin panel); group switches inside Settings keep their drafts.
onBeforeRouteUpdate(async (to, from) => {
  const key = (r: typeof to) => guardKey(String(r.params.section || 'settings'), r.params.sub ? String(r.params.sub) : undefined)
  if (key(to) === key(from)) return true
  return confirmDiscard()
})
onBeforeRouteLeave(() => confirmDiscard())
</script>

<template>
  <div class="adm">
    <TopBar />
    <div class="adm-body">
      <SubNav />
      <main id="adm-main" class="adm-main">
        <SettingsSection v-if="section === 'settings'" key="settings" />
        <LlmSection v-else-if="section === 'llm'" key="llm" />
        <ConfigSection v-else-if="isConfig" :key="`cfg-${section}-${route.params.sub || ''}`" />
        <BackupSection v-else-if="section === 'backup'" key="backup" />
        <UsersSection v-else-if="section === 'users'" key="users" />
        <AuditSection v-else-if="section === 'audit'" key="audit" />
        <SystemSection v-else-if="section === 'system'" key="system" />
      </main>
    </div>
  </div>
</template>
