import { describe, expect, it } from 'vitest'
import { stagesOf, USUAL } from '../src/lib/aiSteps'

const s = (...codes: string[]) => codes.map((code) => ({ code }))
const view = (steps: ReturnType<typeof s>, finished = false) => stagesOf(steps, finished).map((x) => `${x.code}:${x.state}`)

describe('the stages of an AI request', () => {
  it('shows the usual way through before anything has been reported', () => {
    expect(view([])).toEqual(['preparing:current', 'calling_model:todo', 'evaluating:todo', 'previewing:todo'])
  })

  it('marks what is done, what is going on and what is still ahead', () => {
    expect(view(s('preparing'))).toEqual(['preparing:current', 'calling_model:todo', 'evaluating:todo', 'previewing:todo'])
    expect(view(s('preparing', 'calling_model'))).toEqual(['preparing:done', 'calling_model:current', 'evaluating:todo', 'previewing:todo'])
    expect(view(s('preparing', 'calling_model', 'evaluating'))).toEqual(['preparing:done', 'calling_model:done', 'evaluating:current', 'previewing:todo'])
  })

  it('adds the detours when they happen, without dropping the stages still to come', () => {
    expect(view(s('preparing', 'calling_model', 'repairing'))).toEqual(['preparing:done', 'calling_model:done', 'repairing:current', 'evaluating:todo', 'previewing:todo'])
    expect(view(s('preparing', 'calling_model', 'evaluating', 'refining', 'autofix'))).toEqual([
      'preparing:done', 'calling_model:done', 'evaluating:done', 'refining:done', 'autofix:current', 'previewing:todo',
    ])
  })

  it('shows a stage that repeats once, with its latest round, and ignores the model answer', () => {
    const steps = [{ code: 'preparing' }, { code: 'calling_model' }, { code: 'model_answer' }, { code: 'evaluating' }, { code: 'refining', detail: { round: 1 } }, { code: 'evaluating' }, { code: 'refining', detail: { round: 2 } }]
    const v = stagesOf(steps)
    expect(v.filter((x) => x.code === 'refining')).toHaveLength(1)
    expect(v.find((x) => x.code === 'refining')?.round).toBe(2)
    expect(v.some((x) => x.code === 'model_answer')).toBe(false)
    expect(v.filter((x) => x.state === 'current')).toHaveLength(1)
  })

  it('has everything done when the request is over and shows nothing ahead', () => {
    expect(view(s('preparing', 'calling_model', 'evaluating', 'previewing'), true)).toEqual(['preparing:done', 'calling_model:done', 'evaluating:done', 'previewing:done'])
    expect(view(s('preparing', 'calling_model'), true)).toEqual(['preparing:done', 'calling_model:done'])
  })

  it('knows only the stages the server reports', () => {
    expect(stagesOf(s('something_new'))).toEqual(stagesOf([]))
    expect(USUAL.every((c) => stagesOf([]).some((x) => x.code === c))).toBe(true)
  })
})
