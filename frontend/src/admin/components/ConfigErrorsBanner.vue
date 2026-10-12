<script setup lang="ts">
import { computed, ref } from 'vue'
import { TriangleAlert } from 'lucide-vue-next'
import { has, t } from '@/i18n'
import { kindPath } from '../lib/configKinds'
import { useAdminConfig } from '../store'

const store = useAdminConfig()
const all = ref(false)
const shown = computed(() => (all.value ? store.errors : store.errors.slice(0, 3)))

function kindLabel(kind: string): string {
  return has(`admin.kind.${kind}.title`) ? t(`admin.kind.${kind}.title`) : kind
}
</script>

<template>
  <div v-if="store.errors.length" class="banner banner-warn cfg-errors" role="alert">
    <div class="row">
      <TriangleAlert :size="16" />
      <strong>{{ t('admin.errors.title', { n: store.errors.length }) }}</strong>
    </div>
    <p>{{ t('admin.errors.hint') }}</p>
    <ul>
      <li v-for="e in shown" :key="e.kind + e.id">
        <router-link :to="kindPath(e.kind, e.id)"><span class="mono">{{ e.id }}</span> <span class="faint">({{ kindLabel(e.kind) }})</span></router-link>
        <span class="cfg-error-msg">{{ e.error }}</span>
      </li>
    </ul>
    <button v-if="store.errors.length > 3" class="btn btn-sm btn-ghost" @click="all = !all">{{ all ? t('admin.errors.less') : t('admin.errors.more', { n: store.errors.length - 3 }) }}</button>
  </div>
</template>
