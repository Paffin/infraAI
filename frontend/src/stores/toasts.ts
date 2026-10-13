import { defineStore } from 'pinia'
import { ref } from 'vue'
import { t } from '@/i18n'
import { useApp } from './app'

export interface ToastAction { label: string; run: () => void }
export interface Toast { id: number; kind: 'info' | 'ok' | 'error'; text: string; /** how long it stays, ms (the timer bar of the toast runs for it) */ ms: number; action?: ToastAction }

export const useToasts = defineStore('toasts', () => {
  const app = useApp()
  const items = ref<Toast[]>([])
  let seq = 0
  /** How long a notification stays is a setting (`editor.toastMs`, `editor.errorToastMs`, `editor.undoToastMs`). */
  function push(kind: Toast['kind'], text: string, ms = kind === 'error' ? app.editorCfg.errorToastMs : app.editorCfg.toastMs, action?: ToastAction) {
    const id = ++seq
    items.value.push({ id, kind, text, ms, action })
    setTimeout(() => dismiss(id), ms)
  }
  function dismiss(id: number) {
    items.value = items.value.filter((x) => x.id !== id)
  }
  /** What was just done, and the way back: gone with the next thing the user does (only the last action can be undone). */
  function undoable(text: string, run: () => void) {
    push('info', text, app.editorCfg.undoToastMs, { label: t('toast.undo'), run })
  }
  /** The notifications that offer a way back are stale as soon as something else happens. */
  function clearActions() {
    if (items.value.some((x) => x.action)) items.value = items.value.filter((x) => !x.action)
  }
  return {
    items, dismiss, undoable, clearActions,
    info: (text: string) => push('info', text),
    ok: (text: string) => push('ok', text),
    error: (text: string) => push('error', text),
  }
})
