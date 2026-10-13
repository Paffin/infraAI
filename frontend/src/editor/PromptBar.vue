<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { Check, RotateCcw, Sparkles, X } from 'lucide-vue-next'
import { lt, t } from '@/i18n'
import { stagesOf } from '@/lib/aiSteps'
import { aiErrorText, useAi } from '@/stores/ai'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'
import { useUi } from '@/stores/ui'

const app = useApp()
const editor = useEditor()
const ai = useAi()
const ui = useUi()

const text = ref('')
const input = ref<HTMLInputElement | null>(null)
const max = computed(() => app.boot?.ai.maxPromptChars ?? 4000)
const disabled = computed(() => !ui.aiOn || !app.canEdit || editor.readonly)
const empty = computed(() => editor.isEmpty)
const quick = computed(() => app.boot?.app.quickPrompts ?? [])
const stages = computed(() => stagesOf(ai.job?.steps ?? []))
const stageText = (s: { code: string; round?: number }) => (s.round ? `${t('job.' + s.code)} · ${s.round}` : t('job.' + s.code))

// how long the request has been going: the wait is shorter when it is counted
const elapsed = ref(0)
let clock: ReturnType<typeof setInterval> | undefined
watch(() => ai.busy, (b) => {
  clearInterval(clock)
  if (!b) return
  const from = Date.now()
  elapsed.value = 0
  clock = setInterval(() => { elapsed.value = Math.floor((Date.now() - from) / 1000) }, 500)
}, { immediate: true })
onBeforeUnmount(() => clearInterval(clock))
const placeholder = computed(() => (!ui.aiOn ? (ui.aiMode === 'manual' ? t('prompt.hint.manual') : t('editor.mode.ai.off')) : empty.value ? t('prompt.placeholder.empty') : t('prompt.placeholder.edit')))

async function run(kind: 'generate' | 'patch') {
  const v = text.value.trim()
  if (!v || disabled.value || ai.busy) return
  if (await ai.submit(kind, v)) text.value = ''
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); void run(empty.value ? 'generate' : 'patch') }
}

function chip(p: { label: unknown; prompt: unknown }) {
  text.value = lt(p.prompt as string)
  input.value?.focus()
}

watch(() => ui.promptFocus, () => input.value?.focus())
defineExpose({ focus: () => input.value?.focus() })
</script>

<template>
  <section class="promptbar" data-tour="prompt" :aria-label="t('prompt.region')">
    <div class="row main">
      <div class="field-wrap" :class="{ off: disabled }">
        <Sparkles :size="16" class="spark" />
        <input ref="input" v-model="text" class="pin" :maxlength="max" :disabled="disabled" :placeholder="placeholder" :aria-label="t('prompt.generate')" @keydown="onKey" />
        <span v-if="!disabled" class="kbd">⌘ ↵</span>
      </div>
      <button v-if="!empty && !disabled" class="btn regen" :disabled="ai.busy || !text.trim()" :title="t('prompt.regenerate')" @click="run('generate')"><RotateCcw :size="14" />{{ t('prompt.regenerate') }}</button>
      <button class="btn btn-primary go" :disabled="disabled || ai.busy || !text.trim()" @click="run(empty ? 'generate' : 'patch')">{{ empty ? t('prompt.generate') : t('prompt.edit') }}</button>
      <template v-if="!disabled">
        <button v-for="(q, i) in quick" :key="i" class="chip q" :disabled="ai.busy" @click="chip(q)">{{ lt(q.label as string) }}</button>
      </template>
    </div>
    <div v-if="ai.busy" class="status">
      <ol class="stages" :aria-label="t('prompt.running')" data-testid="ai-stages">
        <li v-for="s in stages" :key="s.code" :class="s.state">
          <Check v-if="s.state === 'done'" :size="13" class="ic" />
          <span v-else-if="s.state === 'current'" class="spinner ic" />
          <i v-else class="ic dot" />
          <span class="lb">{{ stageText(s) }}</span>
        </li>
      </ol>
      <span class="grow" /><span class="secs mono">{{ t('common.seconds', { n: elapsed }) }}</span>
      <button class="btn btn-sm" @click="ai.cancel()">{{ t('prompt.cancel') }}</button>
    </div>
    <div v-else-if="ai.error" class="status err" role="alert">
      <span class="grow">{{ aiErrorText(ai.error.code, ai.error.message) }}</span>
      <button class="btn btn-ghost btn-icon" :aria-label="t('common.close')" @click="ai.dismissError()"><X :size="15" /></button>
    </div>
  </section>
</template>

<style scoped>
.promptbar { background: var(--surface); border-bottom: 1px solid var(--line); padding: 10px 20px; flex: none; }
.main { gap: 12px; min-height: 46px; }
.field-wrap { flex: 1; min-width: 200px; display: flex; align-items: center; gap: 10px; height: 46px; padding: 0 14px; border: 1.5px solid var(--accent-line); border-radius: 10px; background: var(--surface); box-shadow: 0 0 0 3px var(--accent-soft); }
.field-wrap:focus-within { border-color: var(--accent); }
.field-wrap.off { border-color: var(--line-strong); box-shadow: none; background: var(--surface-2); }
.spark { color: var(--accent); flex: none; }
.field-wrap.off .spark { color: var(--faint); }
.pin { flex: 1; min-width: 0; border: 0; outline: none; background: transparent; font-size: 14.5px; height: 100%; }
.pin::placeholder { color: var(--faint); }
.go { height: 46px; padding: 0 24px; font-size: 14.5px; border-radius: 9px; }
.regen { height: 46px; }
.q { flex: none; height: 32px; background: var(--surface); }
.q:disabled { opacity: .5; }
.status { display: flex; align-items: center; gap: 10px; margin-top: 8px; font-size: 13px; color: var(--text-2); }
.stages { list-style: none; margin: 0; padding: 0; display: flex; align-items: center; flex-wrap: wrap; gap: 4px 6px; }
.stages li { display: inline-flex; align-items: center; gap: 6px; padding: 3px 10px 3px 8px; border-radius: 999px; border: 1px solid transparent; font-size: 12.5px; transition: color var(--dur-base), background-color var(--dur-base), border-color var(--dur-base); animation: rise-in var(--dur-base) var(--ease-out) both; }
.stages li.done { color: var(--muted); }
.stages li.done .ic { color: var(--ok); }
.stages li.current { color: var(--accent); background: var(--accent-soft); border-color: var(--accent-line); font-weight: 600; }
.stages li.todo { color: var(--faint); }
.stages .ic { flex: none; }
.stages .spinner.ic { width: 12px; height: 12px; }
.stages .dot { width: 5px; height: 5px; margin: 0 4px; border-radius: 50%; background: var(--line-strong); }
.stages li + li::before { content: ''; width: 10px; height: 1px; background: var(--line-strong); margin-right: 2px; }
.secs { font-size: 12px; color: var(--faint); font-variant-numeric: tabular-nums; }
.status.err { color: #7d1a1f; background: var(--crit-bg); border: 1px solid var(--crit-line); border-radius: var(--radius); padding: 4px 6px 4px 12px; }
@media (max-width: 1280px) { .q { display: none; } }
</style>
