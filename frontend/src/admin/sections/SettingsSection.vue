<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { RefreshCw, Search } from 'lucide-vue-next'
import { has, lt, t } from '@/i18n'
import { useDirtyGuard } from '../lib/dirty'
import { settingMatches } from '../lib/settings'
import { useSettingsEditor } from '../lib/useSettingsEditor'
import SettingControl from '../components/SettingControl.vue'
import SettingsSaveBar from '../components/SettingsSaveBar.vue'
import StateBlock from '../components/StateBlock.vue'

const route = useRoute()
const router = useRouter()
const ed = useSettingsEditor()
const query = ref('')

useDirtyGuard(() => ed.dirty.value.length > 0)
onMounted(ed.load)

const active = computed(() => {
  const g = String(route.params.sub || '')
  return ed.groups.value.find((x) => x.group === g)?.group ?? ed.groups.value[0]?.group ?? ''
})

function groupLabel(g: string): string {
  return has(`admin.group.${g}`) ? t(`admin.group.${g}`) : g
}

const dirtyByGroup = computed(() => {
  const m = new Map<string, number>()
  for (const k of ed.dirty.value) {
    const g = ed.byKey.value.get(k)?.group
    if (g) m.set(g, (m.get(g) ?? 0) + 1)
  }
  return m
})

/** Rows to show: the active group, or all matches when searching. */
const sections = computed(() => {
  const q = query.value.trim()
  if (!q) {
    const g = ed.groups.value.find((x) => x.group === active.value)
    return g ? [{ group: g.group, items: g.items }] : []
  }
  return ed.groups.value
    .map((g) => ({ group: g.group, items: g.items.filter((d) => settingMatches(d, q, lt(d.label), lt(d.help))) }))
    .filter((g) => g.items.length)
})

async function save() {
  const ok = await ed.save()
  if (ok) return
  // bring the first failing control into view (switching to its group when needed)
  const key = Object.keys(ed.errors)[0]
  const def = key ? ed.byKey.value.get(key) : undefined
  if (!def) return
  if (!query.value.trim() && def.group !== active.value) router.replace({ path: `/admin/settings/${def.group}` })
  await nextTick()
  document.getElementById(`setting-${key}`)?.scrollIntoView({ block: 'center', behavior: 'smooth' })
}

watch(() => route.params.sub, () => { query.value = '' })

// the strip of groups scrolls when they do not fit: the active one is brought into view and a fade says there is more on that side
const strip = ref<HTMLElement | null>(null)
const edge = ref({ left: false, right: false })
function measureStrip() {
  const el = strip.value
  if (el) edge.value = { left: el.scrollLeft > 2, right: el.scrollLeft + el.clientWidth < el.scrollWidth - 2 }
}
watch([active, () => ed.groups.value.length], async () => {
  await nextTick()
  strip.value?.querySelector<HTMLElement>('.tab.active')?.scrollIntoView({ inline: 'nearest', block: 'nearest' })
  measureStrip()
})
onMounted(() => window.addEventListener('resize', measureStrip))
onBeforeUnmount(() => window.removeEventListener('resize', measureStrip))
</script>

<template>
  <section class="adm-page">
    <header class="adm-head">
      <div class="grow">
        <h1>{{ t('admin.settings.title') }}</h1>
        <p class="muted">{{ t('admin.settings.subtitle') }}</p>
      </div>
      <div class="adm-search">
        <Search :size="15" aria-hidden="true" />
        <input v-model="query" class="input" type="search" :placeholder="t('admin.settings.search')" :aria-label="t('admin.settings.search')" />
      </div>
      <button class="btn btn-icon" :title="t('common.refresh')" :aria-label="t('common.refresh')" :disabled="ed.loading.value || ed.dirty.value.length > 0" @click="ed.load()"><RefreshCw :size="15" /></button>
    </header>

    <StateBlock :loading="ed.loading.value && !ed.defs.value.length" :error="ed.loadError.value" @retry="ed.load()">
      <div v-if="!query.trim()" class="adm-tabs-wrap" :class="{ 'more-left': edge.left, 'more-right': edge.right }">
      <nav ref="strip" class="tabs adm-tabs" :aria-label="t('admin.settings.groups')" @scroll.passive="measureStrip">
        <router-link
          v-for="g in ed.groups.value" :key="g.group" :to="`/admin/settings/${g.group}`" class="tab" :class="{ active: g.group === active }"
          :aria-current="g.group === active ? 'page' : undefined"
        >
          {{ groupLabel(g.group) }}
          <span v-if="g.modified" class="count" :title="t('admin.settings.modifiedCount', { n: g.modified })">{{ g.modified }}</span>
          <span v-if="dirtyByGroup.get(g.group)" class="unsaved-dot" :title="t('admin.settings.unsaved')" />
        </router-link>
      </nav>
      </div>

      <div v-if="!sections.length" class="empty">{{ t('admin.settings.noMatch') }}</div>
      <div v-for="s in sections" :key="s.group" class="card srows">
        <h2 v-if="query.trim()" class="srows-title">{{ groupLabel(s.group) }}</h2>
        <SettingControl
          v-for="d in s.items" :key="d.key" :def="d" :model-value="ed.drafts[d.key]" :error="ed.errors[d.key]" :disabled="ed.saving.value"
          @update:model-value="ed.setDraft(d.key, $event)" @reset="ed.resetToDefault(d.key)"
        />
      </div>
    </StateBlock>

    <SettingsSaveBar :count="ed.dirty.value.length" :saving="ed.saving.value" :status="ed.status.value" @save="save" @discard="ed.discard()" />
  </section>
</template>
