<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { Check, Sparkles } from 'lucide-vue-next'
import { t } from '@/i18n'
import { MOTION_PREFS, type MotionPref } from '@/lib/motion'
import { useMotion } from '@/stores/motion'

const motion = useMotion()
const open = ref(false)
const root = ref<HTMLElement | null>(null)

function pick(p: MotionPref) {
  motion.choose(p)
  open.value = false
}
function onDown(e: MouseEvent) {
  if (open.value && root.value && !root.value.contains(e.target as Node)) open.value = false
}
function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape' && open.value) { open.value = false; e.stopPropagation() }
}
onMounted(() => { document.addEventListener('mousedown', onDown); document.addEventListener('keydown', onKey, true) })
onBeforeUnmount(() => { document.removeEventListener('mousedown', onDown); document.removeEventListener('keydown', onKey, true) })
</script>

<template>
  <div ref="root" class="motion-switch">
    <button class="trigger" :class="{ on: open }" :aria-expanded="open" aria-haspopup="menu" :title="t('motion.title')" :aria-label="t('motion.title')" data-motion-menu @click="open = !open">
      <Sparkles :size="15" />
    </button>
    <Transition name="pop">
      <div v-if="open" class="menu" role="menu" :aria-label="t('motion.title')">
        <div class="head">{{ t('motion.title') }}</div>
        <button v-for="p in MOTION_PREFS" :key="p" class="item" :class="{ cur: motion.choice === p }" role="menuitemradio" :aria-checked="motion.choice === p" :data-pref="p" @click="pick(p)">
          <span class="txt"><b>{{ t('motion.' + p) }}</b><small>{{ t('motion.' + p + '.hint') }}</small></span>
          <Check v-if="motion.choice === p" :size="15" class="tick" />
        </button>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.motion-switch { position: relative; display: inline-flex; }
.trigger { width: 30px; height: 28px; display: inline-flex; align-items: center; justify-content: center; border: 1px solid #3a4256; border-radius: var(--radius-sm); background: transparent; color: var(--on-dark-muted); }
.trigger:hover, .trigger.on { color: #fff; background: var(--ink-3); }
.menu { position: absolute; right: 0; top: calc(100% + 8px); z-index: 120; width: 272px; background: var(--surface); color: var(--text); border: 1px solid var(--line); border-radius: var(--radius); box-shadow: var(--shadow-3); padding: 6px; }
.head { padding: 6px 10px 4px; font-size: 11px; font-weight: 700; letter-spacing: .06em; text-transform: uppercase; color: var(--muted); }
.item { width: 100%; display: flex; align-items: center; gap: 8px; text-align: left; padding: 8px 10px; border: 0; border-radius: var(--radius-sm); background: transparent; color: var(--text); }
.item:hover { background: var(--surface-sunken); }
.item.cur { background: var(--accent-soft); }
.txt { display: flex; flex-direction: column; gap: 1px; flex: 1; min-width: 0; }
.txt b { font-size: 13px; font-weight: 600; }
.txt small { font-size: 11.5px; color: var(--muted); line-height: 1.35; }
.tick { color: var(--accent); flex: none; }
</style>
