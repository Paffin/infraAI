import type { Need } from '@/api/types'
import { useEditor } from '@/stores/editor'
import { byUrgency } from './findings'
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

/**
 * The next (or previous) open finding, in the order the list of checks has it (the worst level first): the checks tab opens on it and the diagram shows what it is
 * about. The keys N and P and the palette's "next finding" are this one function, read when it is called, not when a list of commands was drawn. `false`: no finding is open.
 */
export function stepFinding(dir: 1 | -1): boolean {
  const editor = useEditor()
  const ui = useUi()
  const list = byUrgency(editor.openFindings)
  if (!list.length) return false
  const at = list.findIndex((f) => f.id === ui.findingCursor)
  const next = list[at < 0 ? (dir > 0 ? 0 : list.length - 1) : (at + dir + list.length) % list.length]
  ui.findingCursor = next.id
  ui.tab = 'checks'
  editor.showSubjects(next)
  return true
}
