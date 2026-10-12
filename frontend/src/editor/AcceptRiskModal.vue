<script setup lang="ts">
import { computed, ref } from 'vue'
import type { Finding } from '@/api/types'
import Modal from '@/components/Modal.vue'
import { t } from '@/i18n'
import { errorMessage } from '@/lib/format'
import { findingTitle } from '@/lib/findings'
import { useApp } from '@/stores/app'
import { useEditor } from '@/stores/editor'
import { useToasts } from '@/stores/toasts'

const props = defineProps<{ finding: Finding }>()
const emit = defineEmits<{ close: [] }>()
const app = useApp()
const editor = useEditor()
const toasts = useToasts()

const minChars = computed(() => app.boot?.limits.waiverMinReasonChars ?? 10)
const maxDays = computed(() => app.boot?.limits.waiverMaxDays ?? 365)
const reason = ref('')
const days = ref(Math.min(90, maxDays.value))
const busy = ref(false)
const choices = computed(() => [...new Set([30, 90, 180, 365, maxDays.value].filter((d) => d <= maxDays.value))].sort((a, b) => a - b))
const valid = computed(() => reason.value.trim().length >= minChars.value)

async function submit() {
  if (!valid.value || busy.value) return
  busy.value = true
  try {
    await editor.acceptRisk(props.finding, reason.value.trim(), days.value)
    emit('close')
  } catch (e) {
    toasts.error(errorMessage(e))
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <Modal :title="t('accept.title')" @close="emit('close')">
    <form class="col" style="gap: 14px" @submit.prevent="submit">
      <p style="margin: 0"><b>{{ findingTitle(app.rules.get(finding.ruleId), finding) }}</b></p>
      <p class="muted" style="margin: 0">{{ t('accept.text') }}</p>
      <div class="field">
        <label>{{ t('accept.reason') }}</label>
        <textarea v-model="reason" class="textarea" rows="4" autofocus />
        <span class="hint">{{ t('accept.reason.min', { n: minChars }) }} · {{ reason.trim().length }}</span>
      </div>
      <div class="field" style="max-width: 200px">
        <label>{{ t('accept.days') }}</label>
        <select v-model.number="days" class="select"><option v-for="d in choices" :key="d" :value="d">{{ d }}</option></select>
      </div>
      <button type="submit" class="sr-only" tabindex="-1">ok</button>
    </form>
    <template #footer>
      <button class="btn" @click="emit('close')">{{ t('common.cancel') }}</button>
      <button class="btn btn-primary" :disabled="!valid || busy" @click="submit">{{ t('accept.submit') }}</button>
    </template>
  </Modal>
</template>
