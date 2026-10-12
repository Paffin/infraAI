<script setup lang="ts">
import { computed, nextTick, ref } from 'vue'
import { AlignLeft, CircleAlert, CircleCheck, LocateFixed } from 'lucide-vue-next'
import { t } from '@/i18n'
import { formatJson, lineColToOffset, parseJson, type JsonResult } from '../lib/json'
import { jsonErrorText } from '../lib/jsonMessage'

const props = withDefaults(defineProps<{
  modelValue: string
  /** error message from the server, shown verbatim under the editor */
  error?: string | null
  readonly?: boolean
  height?: string
  label?: string
  /** parse result computed by the parent (avoids parsing twice) */
  result?: JsonResult
  compact?: boolean
  id?: string
  placeholder?: string
}>(), { height: '360px' })

const emit = defineEmits<{ 'update:modelValue': [v: string]; save: [] }>()

const ta = ref<HTMLTextAreaElement | null>(null)
const gutter = ref<HTMLElement | null>(null)

const res = computed<JsonResult>(() => props.result ?? parseJson(props.modelValue))
const lineCount = computed(() => props.modelValue.split('\n').length)
const isEmpty = computed(() => !props.modelValue.trim())
const errLine = computed(() => (res.value.ok ? 0 : res.value.line ?? 0))

const statusText = computed(() => {
  const r = res.value
  if (isEmpty.value) return t('admin.json.empty')
  if (r.ok) return t('admin.json.valid')
  return jsonErrorText(r)
})

function onInput(e: Event) {
  emit('update:modelValue', (e.target as HTMLTextAreaElement).value)
}

function onScroll() {
  if (gutter.value && ta.value) gutter.value.scrollTop = ta.value.scrollTop
}

function onKey(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
    e.preventDefault()
    emit('save')
  }
}

function format() {
  const f = formatJson(props.modelValue)
  if (f !== null) emit('update:modelValue', f)
}

/** Put the caret at `offset` and scroll it into view. */
async function jumpTo(offset: number) {
  await nextTick()
  const el = ta.value
  if (!el) return
  el.focus()
  el.setSelectionRange(offset, offset)
  const line = props.modelValue.slice(0, offset).split('\n').length
  const lh = parseFloat(getComputedStyle(el).lineHeight) || 18
  el.scrollTop = Math.max(0, (line - 3) * lh)
  onScroll()
}

function gotoError() {
  const r = res.value
  if (r.ok) return
  jumpTo(r.offset ?? (r.line ? lineColToOffset(props.modelValue, r.line, r.column) : 0))
}

/** Focus without scrolling: the caret goes to the start (a freshly set value would put it at the end). */
function focus() {
  const el = ta.value
  if (!el) return
  el.focus({ preventScroll: true })
  el.setSelectionRange(0, 0)
  el.scrollTop = 0
  onScroll()
}

defineExpose({ jumpTo, focus })
</script>

<template>
  <div class="jed" :class="{ invalid: !res.ok && !isEmpty, compact }">
    <div class="jed-bar">
      <span class="jed-status" :class="isEmpty ? 'neutral' : res.ok ? 'ok' : 'bad'" :title="!res.ok ? res.native : undefined">
        <CircleCheck v-if="res.ok" :size="14" /><CircleAlert v-else-if="!isEmpty" :size="14" />
        <span aria-live="polite">{{ statusText }}</span>
      </span>
      <span class="spacer" />
      <button v-if="!res.ok && !isEmpty" type="button" class="btn btn-sm btn-ghost" @click="gotoError"><LocateFixed :size="13" />{{ t('admin.json.goto') }}</button>
      <button type="button" class="btn btn-sm btn-ghost" :disabled="!res.ok || readonly" @click="format"><AlignLeft :size="13" />{{ t('admin.json.format') }}</button>
      <slot name="tools" />
    </div>
    <div class="jed-body" :style="{ height }">
      <div ref="gutter" class="jed-gutter" aria-hidden="true"><div v-for="n in lineCount" :key="n" :class="{ err: n === errLine }">{{ n }}</div></div>
      <textarea
        :id="id" ref="ta" class="jed-text" :value="modelValue" :readonly="readonly" :aria-label="label" :aria-invalid="!res.ok && !isEmpty" :placeholder="placeholder"
        wrap="off" spellcheck="false" autocomplete="off" autocapitalize="off"
        @input="onInput" @scroll="onScroll" @keydown="onKey"
      />
    </div>
    <div v-if="error" class="banner banner-err jed-error" role="alert">{{ error }}</div>
  </div>
</template>
