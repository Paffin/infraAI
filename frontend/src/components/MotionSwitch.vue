<script setup lang="ts">
import { Check, Sparkles } from 'lucide-vue-next'
import { t } from '@/i18n'
import { MOTION_PREFS, type MotionPref } from '@/lib/motion'
import { useMotion } from '@/stores/motion'
import HeaderMenu from './HeaderMenu.vue'

const motion = useMotion()
const pick = (p: MotionPref, close: () => void) => { motion.choose(p); close() }
</script>

<template>
  <HeaderMenu class="motion-switch" :icon="Sparkles" :label="t('motion.title')">
    <template #default="{ close }">
      <button v-for="p in MOTION_PREFS" :key="p" class="item" :class="{ cur: motion.choice === p }" role="menuitemradio" :aria-checked="motion.choice === p" :data-pref="p" @click="pick(p, close)">
        <span class="txt"><b>{{ t('motion.' + p) }}</b><small>{{ t('motion.' + p + '.hint') }}</small></span>
        <Check v-if="motion.choice === p" :size="15" class="tick" />
      </button>
    </template>
  </HeaderMenu>
</template>

<style scoped>
.item { width: 100%; display: flex; align-items: center; gap: 8px; text-align: left; padding: 8px 10px; border: 0; border-radius: var(--radius-sm); background: transparent; color: var(--text); }
.item:hover { background: var(--surface-sunken); }
.item.cur { background: var(--accent-soft); }
.txt { display: flex; flex-direction: column; gap: 1px; flex: 1; min-width: 0; }
.txt b { font-size: 13px; font-weight: 600; }
.txt small { font-size: 11.5px; color: var(--muted); line-height: 1.35; }
.tick { color: var(--accent); flex: none; }
</style>
