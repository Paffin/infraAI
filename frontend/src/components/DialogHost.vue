<script setup lang="ts">
import { nextTick, ref, watch } from 'vue'
import { t } from '@/i18n'
import { useDialogs } from '@/stores/dialogs'
import Modal from './Modal.vue'

const d = useDialogs()
const value = ref('')
const field = ref<HTMLInputElement | HTMLTextAreaElement | null>(null)

watch(() => d.promptReq, async (r) => {
  if (!r) return
  value.value = r.value ?? ''
  await nextTick()
  field.value?.focus()
  field.value?.select()
})
</script>

<template>
  <Modal v-if="d.confirmReq" :title="d.confirmReq.title" @close="d.answerConfirm(false)">
    <p v-if="d.confirmReq.text" style="margin: 0; white-space: pre-line">{{ d.confirmReq.text }}</p>
    <template #footer>
      <button class="btn" @click="d.answerConfirm(false)">{{ t('common.cancel') }}</button>
      <button class="btn" :class="d.confirmReq.danger ? 'btn-danger' : 'btn-primary'" @click="d.answerConfirm(true)">{{ d.confirmReq.confirmLabel || t('common.confirm') }}</button>
    </template>
  </Modal>
  <Modal v-if="d.promptReq" :title="d.promptReq.title" @close="d.answerPrompt(null)">
    <form class="col" @submit.prevent="d.answerPrompt(value)">
      <p v-if="d.promptReq.text" class="muted" style="margin: 0">{{ d.promptReq.text }}</p>
      <div class="field">
        <label v-if="d.promptReq.label">{{ d.promptReq.label }}</label>
        <textarea v-if="d.promptReq.multiline" ref="field" v-model="value" class="textarea" rows="4" />
        <input v-else ref="field" v-model="value" class="input" />
      </div>
      <button type="submit" class="sr-only" tabindex="-1">ok</button>
    </form>
    <template #footer>
      <button class="btn" @click="d.answerPrompt(null)">{{ t('common.cancel') }}</button>
      <button class="btn btn-primary" @click="d.answerPrompt(value)">{{ d.promptReq.confirmLabel || t('common.save') }}</button>
    </template>
  </Modal>
</template>
