import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { byUrgency } from '../src/lib/findings'
import { stepFinding } from '../src/lib/navigate'
import { useEditor } from '../src/stores/editor'
import { useUi } from '../src/stores/ui'
import { fixture } from './fixtures'

// The reports are the engine's own (recorded, tests/fixtures/README.md): a clean one, and one with critical findings and risks of three rules.
const clean = fixture('report-three-az').report
const overloaded = fixture('report-overloaded').report

beforeEach(() => {
  vi.useFakeTimers() // `showSubjects` takes the highlight away after a while
  setActivePinia(createPinia())
})
afterEach(() => vi.useRealTimers())

/** The editor with a report on it, and the ids of its open findings in the order the list of checks shows them. */
function opened(report: typeof clean) {
  const editor = useEditor()
  editor.report = report
  return { editor, ui: useUi(), order: byUrgency(editor.openFindings).map((f) => f.id) }
}

describe('the next and the previous finding (the keys N and P, the palette)', () => {
  it('has nowhere to go when no finding is open: it says so and touches nothing', () => {
    const { editor, ui } = opened(clean)
    expect(editor.openFindings).toHaveLength(0)
    expect(stepFinding(1)).toBe(false)
    expect(stepFinding(-1)).toBe(false)
    expect(ui.findingCursor).toBeNull()
    expect(editor.highlight).toBeNull()
  })

  it('starts at the worst finding going forward and at the least urgent going back', () => {
    const { ui, order, editor } = opened(overloaded)
    expect(order.length).toBeGreaterThan(2)
    expect(editor.openFindings.find((f) => f.id === order[0])!.status).toBe('critical')
    expect(stepFinding(1)).toBe(true)
    expect(ui.findingCursor).toBe(order[0])
    ui.findingCursor = null
    expect(stepFinding(-1)).toBe(true)
    expect(ui.findingCursor).toBe(order[order.length - 1])
  })

  it('goes through all of them in the order of the list, round and round, and the way back is the way there', () => {
    const { ui, order } = opened(overloaded)
    const seen: (string | null)[] = []
    for (let i = 0; i < order.length + 1; i++) { stepFinding(1); seen.push(ui.findingCursor) }
    expect(seen).toEqual([...order, order[0]])
    for (let i = 0; i < order.length; i++) stepFinding(-1)
    expect(ui.findingCursor).toBe(order[0])
  })

  it('opens the checks on the finding and shows what it is about on the diagram', () => {
    const { ui, editor, order } = opened(overloaded)
    ui.tab = 'requirements'
    stepFinding(1)
    expect(ui.tab).toBe('checks')
    const f = editor.openFindings.find((x) => x.id === order[0])!
    expect(editor.highlight).toMatchObject({ nodes: f.subjects.nodes, edges: f.subjects.edges, zones: f.subjects.zones })
  })

  it('reads the findings when it is called: a palette that was drawn before an edit steps over the findings of now, not over the ones it was drawn with', () => {
    const { ui, editor, order } = opened(overloaded)
    stepFinding(1)
    expect(ui.findingCursor).toBe(order[0])
    // an edit resolved the finding the cursor is on
    editor.report = { ...overloaded, findings: overloaded.findings.filter((f) => f.id !== order[0]) }
    const now = byUrgency(editor.openFindings).map((f) => f.id)
    expect(now).not.toContain(order[0])
    // the cursor is on a finding that is gone: the walk starts again from the worst
    stepFinding(1)
    expect(ui.findingCursor).toBe(now[0])
  })
})
