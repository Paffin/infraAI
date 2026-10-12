<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { useRoute } from 'vue-router'
import { kindFromRoute } from '../lib/configKinds'
import { useAdminConfig } from '../store'
import ConfigErrorsBanner from '../components/ConfigErrorsBanner.vue'
import ConfigItems from './ConfigItems.vue'
import RulesSection from './RulesSection.vue'
import SingleDocEditor from './SingleDocEditor.vue'

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
    <ConfigItems v-else :key="meta.kind" :kind="meta.kind" />
  </div>
  <div v-else class="empty">404</div>
</template>
