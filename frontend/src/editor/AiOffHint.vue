<script setup lang="ts">
import { computed, ref } from 'vue'
import { Sparkles, X } from 'lucide-vue-next'
import { t } from '@/i18n'
import { useAi } from '@/stores/ai'
import { useApp } from '@/stores/app'

/**
 * With AI off the editor has no prompt bar, no switch and no chat (lib/aiChrome.ts). Whoever can turn it on is told once, in one line, with the way to do it:
 * the administrator. Everybody else sees nothing: they can do nothing about it, and a line that says "unavailable" is a line to read for nothing.
 * "Hide" is remembered per browser; a storage that is not there only means the hint comes back.
 */
const KEY = 'ia_ai_hint_off'
const read = (): boolean => { try { return localStorage.getItem(KEY) === '1' } catch { return false } }

const app = useApp()
const ai = useAi()
const hidden = ref(read())
const shown = computed(() => app.isAdmin && !ai.enabled && !hidden.value)

function hide() {
  hidden.value = true
  try { localStorage.setItem(KEY, '1') } catch { /* the hint will be back on the next visit: nothing to do about it */ }
}
</script>

<template>
  <section v-if="shown" class="aioff" :aria-label="t('ai.off.region')" data-testid="ai-off-hint">
    <Sparkles :size="14" class="ic" aria-hidden="true" />
    <span class="txt">{{ t('ai.off.hint') }}</span>
    <router-link class="go" to="/admin/llm">{{ t('ai.off.hint.link') }}</router-link>
    <button class="x" :title="t('ai.off.hint.dismiss')" :aria-label="t('ai.off.hint.dismiss')" @click="hide"><X :size="14" /></button>
  </section>
</template>

<style scoped>
.aioff { flex: none; display: flex; align-items: center; gap: 10px; min-height: 32px; padding: 4px 20px; background: var(--accent-soft); border-bottom: 1px solid var(--accent-line); color: var(--text-2); font-size: 12.5px; }
.ic { color: var(--accent); flex: none; }
.txt { min-width: 0; }
.go { font-weight: 600; white-space: nowrap; }
.x { margin-left: auto; flex: none; width: 24px; height: 24px; border: 0; border-radius: var(--radius-sm); background: transparent; color: var(--muted); display: inline-flex; align-items: center; justify-content: center; }
.x:hover { background: var(--surface-sunken); color: var(--text); }
</style>
