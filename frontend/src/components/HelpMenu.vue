<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRoute } from 'vue-router'
import { BookOpen, CircleHelp, Compass, Info, Keyboard } from 'lucide-vue-next'
import { t } from '@/i18n'
import { shortcutText } from '@/lib/shortcuts'
import { tourFor } from '@/lib/tour'
import { useApp } from '@/stores/app'
import { useTour } from '@/stores/tour'
import { useUi } from '@/stores/ui'
import AboutModal from './AboutModal.vue'
import HeaderMenu from './HeaderMenu.vue'

// the window of "About" is a sibling of the menu, so the attributes the parent passes (the tour's anchor) go to the menu itself
defineOptions({ inheritAttrs: false })

const app = useApp()
const tour = useTour()
const ui = useUi()
const route = useRoute()

const about = ref(false)
const hasTour = computed(() => !!tourFor(route.name))
const inEditor = computed(() => route.name === 'editor')

function startTour(close: () => void) {
  close()
  tour.request()
}
function shortcuts(close: () => void) {
  close()
  ui.modal = 'shortcuts'
}
function openAbout(close: () => void) {
  close()
  about.value = true
}
</script>

<template>
  <HeaderMenu v-bind="$attrs" class="help-menu" :icon="CircleHelp" :label="t('help.title')" :width="276">
    <template #default="{ close }">
      <button v-if="hasTour" class="item" data-testid="help-tour" @click="startTour(close)">
        <Compass :size="16" class="ic" /><span class="txt"><b>{{ t('help.tour') }}</b><small>{{ t('help.tour.hint') }}</small></span>
      </button>
      <button v-if="inEditor" class="item" @click="shortcuts(close)">
        <Keyboard :size="16" class="ic" /><span class="txt"><b>{{ t('help.shortcuts') }}</b></span><kbd class="kbd">{{ shortcutText('shortcuts') }}</kbd>
      </button>
      <a v-if="app.helpUrl" class="item" :href="app.helpUrl" target="_blank" rel="noopener noreferrer" @click="close()">
        <BookOpen :size="16" class="ic" /><span class="txt"><b>{{ t('help.docs') }}</b></span>
      </a>
      <button class="item" data-testid="help-about" @click="openAbout(close)">
        <Info :size="16" class="ic" /><span class="txt"><b>{{ t('help.about') }}</b><small>{{ t('help.about.hint') }}</small></span>
      </button>
      <button v-if="tour.off" class="item" @click="tour.forget(); close()">
        <span class="ic" /><span class="txt"><small>{{ t('help.tour.on') }}</small></span>
      </button>
    </template>
  </HeaderMenu>
  <AboutModal v-if="about" @close="about = false" />
</template>

<style scoped>
.item { width: 100%; display: flex; align-items: center; gap: 10px; text-align: left; padding: 8px 10px; border: 0; border-radius: var(--radius-sm); background: transparent; color: var(--text); text-decoration: none; }
.item:hover { background: var(--surface-sunken); text-decoration: none; }
.ic { color: var(--muted); flex: none; width: 16px; }
.txt { display: flex; flex-direction: column; gap: 1px; flex: 1; min-width: 0; }
.txt b { font-size: 13px; font-weight: 600; }
.txt small { font-size: 11.5px; color: var(--muted); line-height: 1.35; }
</style>
