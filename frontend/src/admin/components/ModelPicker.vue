<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { ChevronDown, RefreshCw, TriangleAlert } from 'lucide-vue-next'
import * as api from '@/api/endpoints'
import type { LlmModel } from '@/api/types'
import { t } from '@/i18n'
import { errorMessage } from '@/lib/format'
import { fmtContext, fmtPrice, modelCaps } from '../lib/llm'

const props = defineProps<{ modelValue: string; id?: string; disabled?: boolean; invalid?: boolean }>()
const emit = defineEmits<{ 'update:modelValue': [v: string] }>()

const uid = `mp-${Math.random().toString(36).slice(2, 8)}`
const open = ref(false)
const typed = ref('') // what the user typed since opening: the search query
const results = ref<LlmModel[]>([])
const total = ref(0)
const loading = ref(false)
const error = ref<string | null>(null)
const active = ref(-1)
const structuredOnly = ref(false)
const current = ref<LlmModel | null>(null)
const currentChecked = ref(false)
let seq = 0
let timer: ReturnType<typeof setTimeout> | undefined

async function search(refresh = false) {
  const my = ++seq
  loading.value = true
  try {
    const r = await api.admin.models({ q: typed.value.trim(), structured: structuredOnly.value, refresh, limit: 40 })
    if (my !== seq) return
    results.value = r.models
    total.value = r.total
    error.value = null
    active.value = r.models.length && typed.value.trim() ? 0 : -1
  } catch (e) {
    if (my !== seq) return
    results.value = []
    total.value = 0
    error.value = errorMessage(e)
  } finally {
    if (my === seq) loading.value = false
  }
}

function schedule() {
  clearTimeout(timer)
  timer = setTimeout(() => search(), 220)
}

function onFocus() {
  if (props.disabled) return
  open.value = true
  typed.value = ''
  search()
}

function onInput(e: Event) {
  const v = (e.target as HTMLInputElement).value
  emit('update:modelValue', v)
  typed.value = v
  open.value = true
  schedule()
}

function choose(m: LlmModel) {
  emit('update:modelValue', m.id)
  open.value = false
  active.value = -1
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'ArrowDown') {
    e.preventDefault()
    if (!open.value) { open.value = true; search(); return }
    active.value = Math.min(results.value.length - 1, active.value + 1)
    scrollActive()
  } else if (e.key === 'ArrowUp') {
    e.preventDefault()
    active.value = Math.max(0, active.value - 1)
    scrollActive()
  } else if (e.key === 'Enter') {
    if (open.value && active.value >= 0 && results.value[active.value]) {
      e.preventDefault()
      choose(results.value[active.value])
    } else {
      open.value = false
    }
  } else if (e.key === 'Escape' && open.value) {
    e.stopPropagation()
    open.value = false
  }
}

function scrollActive() {
  document.getElementById(`${uid}-opt-${active.value}`)?.scrollIntoView({ block: 'nearest' })
}

const root = ref<HTMLElement | null>(null)

/** Close when the focus leaves the whole widget (the filter checkbox inside the list may take it). */
function onFocusOut(e: FocusEvent) {
  const next = e.relatedTarget as Node | null
  if (next && root.value?.contains(next)) return
  setTimeout(() => { open.value = false }, 100)
}

const infoTimer = ref<ReturnType<typeof setTimeout>>()
async function lookup(v: string) {
  const id = v.trim()
  current.value = null
  currentChecked.value = false
  if (!id) return
  try {
    const r = await api.admin.models({ q: id, limit: 25 })
    if (id !== props.modelValue.trim()) return
    current.value = r.models.find((m) => m.id === id) ?? null
    currentChecked.value = true
  } catch {
    currentChecked.value = false // catalog unavailable: say nothing about the model
  }
}

watch(() => props.modelValue, (v) => {
  clearTimeout(infoTimer.value)
  infoTimer.value = setTimeout(() => lookup(v), 400)
}, { immediate: true })
watch(structuredOnly, () => { if (open.value) search() })
onBeforeUnmount(() => { clearTimeout(timer); clearTimeout(infoTimer.value) })

const caps = computed(() => (current.value ? modelCaps(current.value) : null))
const listId = `${uid}-list`
</script>

<template>
  <div ref="root" class="mp" @focusout="onFocusOut">
    <div class="mp-field">
      <input
        :id="id" class="input mono" :class="{ 'is-invalid': invalid }" :value="modelValue" :disabled="disabled"
        role="combobox" aria-autocomplete="list" :aria-expanded="open" :aria-controls="listId" :aria-activedescendant="active >= 0 ? `${uid}-opt-${active}` : undefined"
        autocomplete="off" spellcheck="false" :placeholder="t('admin.model.placeholder')"
        @focus="onFocus" @input="onInput" @keydown="onKey"
      />
      <ChevronDown class="mp-caret" :size="16" aria-hidden="true" />
    </div>

    <div v-if="open" :id="listId" class="mp-pop" role="listbox" :aria-label="t('admin.model.catalog')" @mousedown.prevent>
      <div v-if="loading && !results.length" class="mp-note"><span class="spinner" /> {{ t('admin.loading') }}</div>
      <div v-else-if="error" class="mp-note mp-err"><TriangleAlert :size="14" /> {{ t('admin.model.unavailable') }} <span class="faint">{{ error }}</span></div>
      <div v-else-if="!results.length" class="mp-note">{{ t('admin.model.none') }}</div>
      <div
        v-for="(m, i) in results" :id="`${uid}-opt-${i}`" :key="m.id" class="mp-opt" :class="{ on: i === active }"
        role="option" :aria-selected="m.id === modelValue" @mousedown.prevent="choose(m)" @mousemove="active = i"
      >
        <div class="mp-main">
          <span class="mono mp-id">{{ m.id }}</span>
          <span class="mp-name truncate">{{ m.name }}</span>
        </div>
        <div class="mp-meta">
          <span v-if="m.structuredOutputs" class="tag tag-accent" :title="t('admin.model.structuredHint')">{{ t('admin.model.structured') }}</span>
          <span class="mp-ctx" :title="t('admin.model.context')">{{ fmtContext(m.contextLength) }}</span>
          <span class="mp-price" :title="t('admin.model.priceHint')">{{ fmtPrice(m.promptPerMillion) }} / {{ fmtPrice(m.completionPerMillion) }}</span>
        </div>
      </div>
      <div class="mp-foot">
        <label class="checkbox"><input v-model="structuredOnly" type="checkbox" tabindex="-1" /> {{ t('admin.model.onlyStructured') }}</label>
        <span class="faint grow">{{ t('admin.model.shown', { n: results.length, total }) }}</span>
        <button type="button" class="btn btn-sm btn-ghost" tabindex="-1" :disabled="loading" @mousedown.prevent="search(true)"><RefreshCw :size="13" />{{ t('admin.model.refresh') }}</button>
      </div>
    </div>

    <div v-if="current && !open" class="mp-info">
      <span>{{ current.name }}</span>
      <span class="faint">{{ t('admin.model.ctx', { n: fmtContext(current.contextLength) }) }}</span>
      <span class="faint">{{ t('admin.model.price', { input: fmtPrice(current.promptPerMillion), output: fmtPrice(current.completionPerMillion) }) }}</span>
      <span v-if="caps?.structured" class="tag tag-ok">{{ t('admin.model.structured') }}</span>
      <span v-else class="tag tag-nodata">{{ t('admin.model.noStructured') }}</span>
      <span v-if="caps && !caps.temperature" class="tag tag-nodata" :title="t('admin.model.noTemperatureHint')">{{ t('admin.model.noTemperature') }}</span>
    </div>
    <div v-else-if="currentChecked && modelValue.trim() && !open" class="mp-info mp-warn"><TriangleAlert :size="13" /> {{ t('admin.model.notFound') }}</div>
  </div>
</template>
