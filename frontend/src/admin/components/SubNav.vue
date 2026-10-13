<script setup lang="ts">
import { computed } from 'vue'
import { useRoute } from 'vue-router'
import { ArrowUpCircle } from 'lucide-vue-next'
import { t } from '@/i18n'
import { kindMeta } from '../lib/configKinds'
import { activeNavId, NAV, type NavItem } from '../lib/nav'
import { useAdminConfig } from '../store'

const route = useRoute()
const store = useAdminConfig()
const current = computed(() => activeNavId(String(route.params.section || 'settings'), route.params.sub ? String(route.params.sub) : undefined))
const parentOf = computed(() => current.value.split('/')[0])

function counter(item: NavItem) {
  return item.kind ? store.counter(item.kind) : undefined
}
/** A single document (engine settings, common properties) is one item: a count of one tells nothing. */
function counted(item: NavItem): boolean {
  return !!item.kind && !kindMeta(item.kind)?.single
}
function errorsOf(item: NavItem): number {
  return item.kind ? store.errors.filter((e) => e.kind === item.kind).length : 0
}
function isActive(item: NavItem): boolean {
  return item.children ? parentOf.value === item.id : current.value === item.id
}
</script>

<template>
  <nav class="subnav" :aria-label="t('admin.nav.label')">
    <div v-for="g in NAV" :key="g.id" class="subnav-group">
      <div class="subnav-title">{{ t(g.label) }}</div>
      <template v-for="item in g.items" :key="item.id">
        <router-link :to="item.to" class="subnav-link" :class="{ on: isActive(item) }" :aria-current="isActive(item) ? 'page' : undefined">
          <component :is="item.icon" v-if="item.icon" :size="16" class="subnav-icon" />
          <span class="grow truncate">{{ t(item.label) }}</span>
          <template v-if="item.kind">
            <span v-if="errorsOf(item)" class="dot crit" :title="t('admin.nav.errors', { n: errorsOf(item) })" />
            <ArrowUpCircle v-if="counter(item)?.updatesAvailable" :size="13" class="subnav-upd" :title="t('admin.nav.updates', { n: counter(item)?.updatesAvailable })" />
            <span v-if="counter(item)?.modified" class="dot accent" :title="t('admin.nav.modified', { n: counter(item)?.modified })" />
            <span v-if="counter(item) && counted(item)" class="subnav-count">{{ counter(item)?.count }}</span>
          </template>
        </router-link>
        <div v-if="item.children" class="subnav-children">
          <router-link v-for="c in item.children" :key="c.id" :to="c.to" class="subnav-link sub" :class="{ on: current === c.id }" :aria-current="current === c.id ? 'page' : undefined">
            <span class="grow truncate">{{ t(c.label) }}</span>
            <span v-if="errorsOf(c)" class="dot crit" :title="t('admin.nav.errors', { n: errorsOf(c) })" />
            <ArrowUpCircle v-if="counter(c)?.updatesAvailable" :size="13" class="subnav-upd" :title="t('admin.nav.updates', { n: counter(c)?.updatesAvailable })" />
            <span v-if="counter(c)?.modified" class="dot accent" :title="t('admin.nav.modified', { n: counter(c)?.modified })" />
            <span v-if="counter(c) && counted(c)" class="subnav-count">{{ counter(c)?.count }}</span>
          </router-link>
        </div>
      </template>
    </div>
  </nav>
</template>
