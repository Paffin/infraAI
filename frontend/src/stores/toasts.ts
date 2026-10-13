import { defineStore } from 'pinia'
import { ref } from 'vue'
import { useApp } from './app'

export interface Toast { id: number; kind: 'info' | 'ok' | 'error'; text: string }

export const useToasts = defineStore('toasts', () => {
  const app = useApp()
  const items = ref<Toast[]>([])
  let seq = 0
  /** How long a notification stays is a setting (`editor.toastMs`, `editor.errorToastMs`). */
  function push(kind: Toast['kind'], text: string, ms = kind === 'error' ? app.editorCfg.errorToastMs : app.editorCfg.toastMs) {
    const id = ++seq
    items.value.push({ id, kind, text })
    setTimeout(() => dismiss(id), ms)
  }
  function dismiss(id: number) {
    items.value = items.value.filter((x) => x.id !== id)
  }
  return {
    items, dismiss,
    info: (text: string) => push('info', text),
    ok: (text: string) => push('ok', text),
    error: (text: string) => push('error', text),
  }
})
