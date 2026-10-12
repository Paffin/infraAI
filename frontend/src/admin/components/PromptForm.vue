<script setup lang="ts">
import { computed, ref } from 'vue'
import { TriangleAlert } from 'lucide-vue-next'
import { t } from '@/i18n'
import { isPlainObject, parseJson, pretty } from '../lib/json'
import { promptVars, unknownPlaceholders } from '../lib/configKinds'

const props = defineProps<{ modelValue: string; promptId: string }>()
const emit = defineEmits<{ 'update:modelValue': [v: string] }>()

const area = ref<HTMLTextAreaElement | null>(null)
const parsed = computed(() => parseJson(props.modelValue))
const obj = computed<Record<string, unknown> | null>(() => (parsed.value.ok && isPlainObject(parsed.value.value) ? parsed.value.value : null))

const system = computed(() => (typeof obj.value?.system === 'string' ? obj.value.system : ''))
const role = computed(() => (typeof obj.value?.role === 'string' ? obj.value.role : ''))
const title = computed(() => obj.value?.title)
const titleLangs = computed(() => {
  const keys = isPlainObject(title.value) ? Object.keys(title.value) : []
  return [...new Set(['ru', 'en', ...keys])]
})
const vars = computed(() => promptVars(props.promptId))
const unknown = computed(() => unknownPlaceholders(system.value, vars.value))
const used = computed(() => new Set(vars.value.filter((v) => system.value.includes(`{{${v}}}`))))
const approxTokens = computed(() => Math.round(system.value.length / 4))

function patch(key: string, value: unknown) {
  if (!obj.value) return
  emit('update:modelValue', pretty({ ...obj.value, [key]: value }))
}

function titleValue(l: string): string {
  const x = title.value
  if (isPlainObject(x)) return typeof x[l] === 'string' ? (x[l] as string) : ''
  return typeof x === 'string' ? x : ''
}

function setTitle(lang: string, v: string) {
  const cur = isPlainObject(title.value) ? title.value : typeof title.value === 'string' ? { ru: title.value, en: title.value } : {}
  patch('title', { ...cur, [lang]: v })
}

function ph(name: string): string {
  return `{{${name}}}`
}

function insert(name: string) {
  const el = area.value
  const token = ph(name)
  if (!el) { patch('system', system.value + token); return }
  const a = el.selectionStart ?? system.value.length
  const b = el.selectionEnd ?? a
  patch('system', system.value.slice(0, a) + token + system.value.slice(b))
  requestAnimationFrame(() => { el.focus(); el.setSelectionRange(a + token.length, a + token.length) })
}
</script>

<template>
  <div v-if="!obj" class="banner banner-warn"><TriangleAlert :size="14" /> {{ t('admin.prompt.fixJson') }}</div>
  <div v-else class="pform">
    <div class="pform-grid">
      <div class="field">
        <label>{{ t('admin.prompt.role') }}</label>
        <select class="select" :value="role" @change="patch('role', ($event.target as HTMLSelectElement).value)">
          <option value="planner">{{ t('admin.llm.role.planner') }} (planner)</option>
          <option value="fast">{{ t('admin.llm.role.fast') }} (fast)</option>
          <option v-if="role && role !== 'planner' && role !== 'fast'" :value="role">{{ role }}</option>
        </select>
        <span class="hint">{{ t('admin.prompt.roleHint') }}</span>
      </div>
      <div v-for="l in titleLangs" :key="l" class="field">
        <label :for="`pt-${l}`">{{ t('admin.prompt.title', { lang: l }) }}</label>
        <input :id="`pt-${l}`" class="input" :value="titleValue(l)" @input="setTitle(l, ($event.target as HTMLInputElement).value)" />
      </div>
    </div>

    <div class="field">
      <div class="row">
        <label for="prompt-system" class="grow">{{ t('admin.prompt.system') }}</label>
        <span class="faint">{{ t('admin.prompt.chars', { n: system.length, tokens: approxTokens }) }}</span>
      </div>
      <textarea id="prompt-system" ref="area" class="textarea mono pform-text" rows="20" spellcheck="false" :value="system" @input="patch('system', ($event.target as HTMLTextAreaElement).value)" />
    </div>

    <div class="pform-vars">
      <span class="label">{{ t('admin.prompt.placeholders') }}</span>
      <p class="hint">{{ t('admin.prompt.placeholdersHint') }}</p>
      <div class="row wrap">
        <button v-for="v in vars" :key="v" type="button" class="chip mono" :class="{ used: used.has(v) }" :title="t('admin.prompt.insert')" @click="insert(v)">{{ ph(v) }}</button>
      </div>
      <div v-if="unknown.length" class="banner banner-warn" role="status" style="margin-top: 8px">
        <TriangleAlert :size="14" /> {{ t('admin.prompt.unknown') }}
        <span v-for="u in unknown" :key="u" class="mono"> {{ ph(u) }}</span>
      </div>
    </div>
  </div>
</template>
