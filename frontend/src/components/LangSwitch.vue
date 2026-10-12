<script setup lang="ts">
import { lang, setLang } from '@/i18n'
import { useApp } from '@/stores/app'
const app = useApp()
</script>

<template>
  <div v-if="app.languages.length > 1" class="lang-switch" role="group" aria-label="Language">
    <button v-for="l in app.languages" :key="l" :class="{ on: lang === l }" @click="setLang(l)">{{ l.toUpperCase() }}</button>
  </div>
</template>

<style scoped>
.lang-switch { display: inline-flex; border: 1px solid #3a4256; border-radius: var(--radius-sm); overflow: hidden; }
.lang-switch button { background: transparent; border: 0; color: var(--on-dark-muted); font-size: 11.5px; font-weight: 600; padding: 0 9px; height: 28px; letter-spacing: .04em; }
.lang-switch button:hover { color: var(--on-dark); }
.lang-switch button.on { background: var(--ink-3); color: #fff; }
</style>
