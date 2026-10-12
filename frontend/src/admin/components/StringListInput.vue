<script setup lang="ts">
import { ref } from 'vue'
import { X } from 'lucide-vue-next'
import { t } from '@/i18n'
import { splitList } from '../lib/settings'

const props = defineProps<{ modelValue: string[]; id?: string; disabled?: boolean; ariaLabel?: string }>()
const emit = defineEmits<{ 'update:modelValue': [v: string[]] }>()

const draft = ref('')

function add(raw: string) {
  const items = splitList(raw).filter((x) => !props.modelValue.includes(x))
  if (items.length) emit('update:modelValue', [...props.modelValue, ...items])
  draft.value = ''
}

function remove(i: number) {
  emit('update:modelValue', props.modelValue.filter((_, k) => k !== i))
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'Enter' || e.key === ',') {
    e.preventDefault()
    add(draft.value)
  } else if (e.key === 'Backspace' && !draft.value && props.modelValue.length) {
    remove(props.modelValue.length - 1)
  }
}

function onPaste(e: ClipboardEvent) {
  const text = e.clipboardData?.getData('text') ?? ''
  if (/[\n,;]/.test(text)) {
    e.preventDefault()
    add(draft.value + text)
  }
}
</script>

<template>
  <div class="chips-input" :class="{ disabled }">
    <span v-for="(item, i) in modelValue" :key="item" class="chip mono">
      {{ item }}
      <button type="button" class="chip-x" :disabled="disabled" :aria-label="t('admin.list.remove', { item })" @click="remove(i)"><X :size="12" /></button>
    </span>
    <input
      :id="id" v-model="draft" class="chips-field" :disabled="disabled" :aria-label="ariaLabel" :placeholder="modelValue.length ? '' : t('admin.list.placeholder')"
      @keydown="onKey" @blur="draft.trim() && add(draft)" @paste="onPaste"
    />
  </div>
</template>
