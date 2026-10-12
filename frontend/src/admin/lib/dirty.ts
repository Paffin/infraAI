// Unsaved-changes guard shared by the admin sections: every editor with pending edits registers a
// predicate; AdminView asks before the route changes, the browser asks before the tab closes.

import { onBeforeUnmount, onMounted } from 'vue'
import { t } from '@/i18n'
import { useDialogs } from '@/stores/dialogs'

const sources = new Set<() => boolean>()

export function anyDirty(): boolean {
  for (const f of sources) {
    if (f()) return true
  }
  return false
}

function beforeUnload(e: BeforeUnloadEvent) {
  if (anyDirty()) {
    e.preventDefault()
    e.returnValue = ''
  }
}

/** Register `isDirty` for the lifetime of the calling component. */
export function useDirtyGuard(isDirty: () => boolean) {
  onMounted(() => {
    sources.add(isDirty)
    window.addEventListener('beforeunload', beforeUnload)
  })
  onBeforeUnmount(() => {
    sources.delete(isDirty)
    if (!sources.size) window.removeEventListener('beforeunload', beforeUnload)
  })
}

/** Ask the user whether pending changes may be thrown away. Resolves true when there is nothing to lose. */
export async function confirmDiscard(): Promise<boolean> {
  if (!anyDirty()) return true
  return useDialogs().confirm({
    title: t('admin.unsaved.title'),
    text: t('admin.unsaved.text'),
    confirmLabel: t('admin.unsaved.leave'),
    danger: true,
  })
}
