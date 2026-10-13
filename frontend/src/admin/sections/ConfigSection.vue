<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { useRoute } from 'vue-router'
import { t } from '@/i18n'
import { kindFromRoute } from '../lib/configKinds'
import { useAdminConfig } from '../store'
import ConfigErrorsBanner from '../components/ConfigErrorsBanner.vue'
import ConfigItems from './ConfigItems.vue'
import RulesSection from './RulesSection.vue'
import SingleDocEditor from './SingleDocEditor.vue'
import VersionsSection from './VersionsSection.vue'

const route = useRoute()
const store = useAdminConfig()
const meta = computed(() => kindFromRoute(String(route.params.section || ''), route.params.sub ? String(route.params.sub) : undefined))
onMounted(() => { if (!store.loaded) store.load() })
</script>

<template>
  <div v-if="meta" class="cfg-section">
    <ConfigErrorsBanner class="cfg-banner" />
    <SingleDocEditor v-if="meta.single" :key="meta.kind" :kind="meta.kind" />
    <RulesSection v-else-if="meta.kind === 'rule'" />
    <VersionsSection v-else-if="meta.kind === 'catalog.versions'" />
    <ConfigItems v-else :key="meta.kind" :kind="meta.kind" />
  </div>
  <div v-else class="empty notfound" data-testid="admin-notfound">
    <h3>{{ t('admin.notFound.title') }}</h3>
    <p>{{ t('admin.notFound.text') }}</p>
    <router-link class="btn btn-primary" :to="{ name: 'admin' }">{{ t('admin.notFound.back') }}</router-link>
  </div>
</template>

<style scoped>
.notfound { display: flex; flex-direction: column; align-items: center; gap: 10px; padding: 72px 16px; }
.notfound h3 { margin: 0; font-size: 18px; color: var(--text); }
.notfound p { margin: 0 0 6px; max-width: 460px; line-height: 1.55; }
</style>
