<script setup lang="ts">
import { computed } from 'vue'
import { RotateCcw, Trash2, Undo2 } from 'lucide-vue-next'
import type { SettingDef } from '@/api/types'
import { lt, t } from '@/i18n'
import { differsFromDefault, formatDefault, isDirty, isSecretDraft, type EditValue, type SecretDraft } from '../lib/settings'
import JsonEditor from './JsonEditor.vue'
import ModelPicker from './ModelPicker.vue'
import StringListInput from './StringListInput.vue'

const props = defineProps<{ def: SettingDef; modelValue: EditValue | undefined; error?: string; disabled?: boolean }>()
const emit = defineEmits<{ 'update:modelValue': [v: EditValue]; reset: [] }>()

const inputId = computed(() => `set-${props.def.key}`)
const str = computed(() => (typeof props.modelValue === 'string' ? props.modelValue : ''))
const bool = computed(() => props.modelValue === true)
const list = computed(() => (Array.isArray(props.modelValue) ? props.modelValue : []))
const secret = computed<SecretDraft>(() => (isSecretDraft(props.modelValue) ? props.modelValue : { text: '', clear: false }))
const dirty = computed(() => isDirty(props.def, props.modelValue))
const differs = computed(() => differsFromDefault(props.def, props.modelValue))
const stored = computed(() => (props.def.type === 'secret' ? props.def.secretSource === 'database' : !!props.def.modified))
const labels = computed(() => ({ yes: t('common.yes'), no: t('common.no'), empty: t('admin.settings.emptyValue') }))
const defaultText = computed(() => formatDefault(props.def, labels.value))
const range = computed(() => {
  const { min, max } = props.def
  if (min != null && max != null) return t('admin.settings.range', { min, max })
  if (min != null) return t('admin.settings.min', { min })
  if (max != null) return t('admin.settings.max', { max })
  return ''
})
const helpText = computed(() => lt(props.def.help))
const secretSourceKey = computed(() => `admin.settings.secret.${props.def.secretSource ?? 'none'}`)

function setSecret(patch: Partial<SecretDraft>) {
  emit('update:modelValue', { ...secret.value, ...patch })
}
</script>

<template>
  <div :id="`setting-${def.key}`" class="srow" :class="{ dirty, invalid: !!error }">
    <div class="srow-info">
      <label :for="inputId" class="srow-label">{{ lt(def.label) }}</label>
      <div class="srow-key">
        <code>{{ def.key }}</code>
        <span v-if="dirty" class="tag tag-risk">{{ t('admin.settings.unsaved') }}</span>
        <span v-else-if="stored" class="tag tag-accent">{{ t('admin.settings.modified') }}</span>
      </div>
      <p v-if="helpText" class="srow-help">{{ helpText }}</p>
    </div>

    <div class="srow-ctl">
      <!-- string -->
      <input v-if="def.type === 'string'" :id="inputId" class="input" :class="{ 'is-invalid': error }" :value="str" :disabled="disabled" autocomplete="off" @input="emit('update:modelValue', ($event.target as HTMLInputElement).value)" />

      <!-- number -->
      <div v-else-if="def.type === 'number'" class="row">
        <input
          :id="inputId" class="input narrow" :class="{ 'is-invalid': error }" type="number" step="any" :min="def.min ?? undefined" :max="def.max ?? undefined" inputmode="decimal"
          :value="str" :disabled="disabled" @input="emit('update:modelValue', ($event.target as HTMLInputElement).value)"
        />
        <span v-if="range" class="faint">{{ range }}</span>
      </div>

      <!-- boolean -->
      <div v-else-if="def.type === 'boolean'" class="row">
        <button :id="inputId" type="button" class="switch" role="switch" :aria-checked="bool" :disabled="disabled" @click="emit('update:modelValue', !bool)"><span class="switch-knob" /></button>
        <span class="muted">{{ bool ? t('admin.settings.on') : t('admin.settings.off') }}</span>
      </div>

      <!-- enum -->
      <select v-else-if="def.type === 'enum'" :id="inputId" class="select" :class="{ 'is-invalid': error }" :value="str" :disabled="disabled" @change="emit('update:modelValue', ($event.target as HTMLSelectElement).value)">
        <option v-for="o in def.options ?? []" :key="o" :value="o">{{ o === '' ? t('admin.settings.emptyOption') : o }}</option>
      </select>

      <!-- string_list -->
      <StringListInput v-else-if="def.type === 'string_list'" :id="inputId" :model-value="list" :disabled="disabled" :aria-label="lt(def.label)" @update:model-value="emit('update:modelValue', $event)" />

      <!-- json -->
      <JsonEditor v-else-if="def.type === 'json'" :id="inputId" :model-value="str" compact height="170px" :label="lt(def.label)" :readonly="disabled" @update:model-value="emit('update:modelValue', $event)" />

      <!-- model -->
      <ModelPicker v-else-if="def.type === 'model'" :id="inputId" :model-value="str" :disabled="disabled" :invalid="!!error" @update:model-value="emit('update:modelValue', $event)" />

      <!-- secret: write-only -->
      <div v-else-if="def.type === 'secret'" class="col secret">
        <div class="row">
          <span class="tag" :class="def.secretSource === 'none' ? 'tag-nodata' : 'tag-ok'">{{ def.secretSource === 'none' ? t('admin.settings.secret.notSet') : t('admin.settings.secret.set') }}</span>
          <span class="muted">{{ t(secretSourceKey) }}</span>
        </div>
        <div class="row">
          <input
            :id="inputId" class="input mono grow" type="password" autocomplete="new-password" spellcheck="false" :disabled="disabled || secret.clear"
            :value="secret.text" :placeholder="t('admin.settings.secret.placeholder')" @input="setSecret({ text: ($event.target as HTMLInputElement).value })"
          />
          <button v-if="def.secretSource === 'database' && !secret.clear" type="button" class="btn" :disabled="disabled" @click="setSecret({ clear: true, text: '' })"><Trash2 :size="14" />{{ t('admin.settings.secret.clear') }}</button>
          <button v-else-if="secret.clear" type="button" class="btn" :disabled="disabled" @click="setSecret({ clear: false })"><Undo2 :size="14" />{{ t('admin.settings.secret.undo') }}</button>
        </div>
        <div v-if="secret.clear" class="hint">{{ t('admin.settings.secret.willClear') }}</div>
        <div v-else-if="secret.text.trim()" class="hint">{{ t('admin.settings.secret.willReplace') }}</div>
      </div>

      <div v-if="error" class="srow-err" role="alert">{{ error }}</div>

      <div v-if="def.type !== 'secret'" class="srow-meta">
        <span class="faint">{{ t('admin.settings.default') }}: <code v-if="def.type !== 'boolean'">{{ defaultText }}</code><span v-else>{{ defaultText }}</span></span>
        <button v-if="differs" type="button" class="btn btn-sm btn-ghost" :disabled="disabled" @click="emit('reset')"><RotateCcw :size="13" />{{ t('admin.settings.resetDefault') }}</button>
      </div>
    </div>
  </div>
</template>
