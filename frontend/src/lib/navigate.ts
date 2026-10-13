import type { Need } from '@/api/types'
import { useEditor } from '@/stores/editor'
import { useUi } from '@/stores/ui'

/**
 * Send the user to a field that has to be filled in: the requirements form, or the inspector of the element
 * (selected and brought into view). The target form opens the right group, focuses the input and flashes it once.
 */
export function focusNeed(need: Need) {
  const editor = useEditor()
  const ui = useUi()
  if (need.target === 'requirements') {
    ui.tab = 'requirements'
  } else if (need.id) {
    editor.select({ type: need.target, id: need.id })
    editor.revealSelection()
  }
  ui.focusRequest = { need, token: Date.now() }
}
