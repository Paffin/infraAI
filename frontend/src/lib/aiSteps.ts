// The stages of an AI request as the person waiting sees them: what is done, what is going on, what is still to come.
// The server reports steps as it takes them (`AiStep.code`); the usual way through is fixed, the detours (repairing the
// model's answer, refining by the engine's findings, safe fixes) appear only when they happen.

export interface AiStepLike { code: string; detail?: unknown }
export type StageState = 'done' | 'current' | 'todo'
export interface Stage { code: string; state: StageState; /** the round of a stage that repeats (the second repair) */ round?: number }

/** Every stage there is, in the order a request goes through them. */
export const ORDER = ['preparing', 'calling_model', 'repairing', 'evaluating', 'refining', 'autofix', 'previewing'] as const
/** The stages every request goes through: shown ahead of time as what is still to come. */
export const USUAL = ['preparing', 'calling_model', 'evaluating', 'previewing'] as const

const rank = (code: string) => ORDER.indexOf(code as (typeof ORDER)[number])

/**
 * The stages to show: those taken so far (the last one is current, or all done when the request is over), then the usual
 * ones that are still ahead. A stage that happens again (a second round of refining) is shown once, with its latest round.
 */
export function stagesOf(steps: AiStepLike[], finished = false): Stage[] {
  const taken: Stage[] = []
  for (const s of steps) {
    if (s.code === 'model_answer' || rank(s.code) < 0) continue
    const round = (s.detail as { round?: number } | null | undefined)?.round
    const at = taken.findIndex((x) => x.code === s.code)
    if (at >= 0) taken[at] = { ...taken[at], round }
    else taken.push({ code: s.code, state: 'done', round })
  }
  // the one that was reported last is where the request is now
  const last = steps.filter((s) => s.code !== 'model_answer' && rank(s.code) >= 0).pop()?.code
  for (const st of taken) st.state = !finished && st.code === last ? 'current' : 'done'
  if (finished) return taken
  const here = last ? rank(last) : -1
  const ahead: Stage[] = USUAL.filter((c) => !taken.some((x) => x.code === c) && rank(c) > here).map((code) => ({ code, state: 'todo' as const }))
  if (!taken.length) return ahead.map((s, i) => (i === 0 ? { ...s, state: 'current' as const } : s))
  return [...taken, ...ahead]
}
