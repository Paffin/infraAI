import { defineStore } from 'pinia'
import { ref } from 'vue'

export interface ConfirmRequest {
  title: string
  text?: string
  confirmLabel?: string
  danger?: boolean
  resolve: (ok: boolean) => void
}

export interface PromptRequest {
  title: string
  text?: string
  label?: string
  value?: string
  multiline?: boolean
  confirmLabel?: string
  resolve: (value: string | null) => void
}

/** Promise-based replacements for window.confirm / window.prompt, rendered by <DialogHost/>. */
export const useDialogs = defineStore('dialogs', () => {
  const confirmReq = ref<ConfirmRequest | null>(null)
  const promptReq = ref<PromptRequest | null>(null)

  function confirm(o: Omit<ConfirmRequest, 'resolve'>): Promise<boolean> {
    return new Promise((resolve) => { confirmReq.value = { ...o, resolve } })
  }
  function prompt(o: Omit<PromptRequest, 'resolve'>): Promise<string | null> {
    return new Promise((resolve) => { promptReq.value = { ...o, resolve } })
  }
  function answerConfirm(ok: boolean) {
    confirmReq.value?.resolve(ok)
    confirmReq.value = null
  }
  function answerPrompt(v: string | null) {
    promptReq.value?.resolve(v)
    promptReq.value = null
  }
  return { confirmReq, promptReq, confirm, prompt, answerConfirm, answerPrompt }
})
