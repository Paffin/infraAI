import { shallowRef } from 'vue'
import { ApiError } from '@/api/client'
import * as api from '@/api/endpoints'
import type { Architecture } from '@/api/types'
import { initialQueue, step, type SaveConfig, type SaveEffect, type SaveEvent, type SaveQueue } from './saveQueue'

// The one place of the save path that has a timer and a request. The rules (what follows what, what is retried, what is a conflict) are `lib/saveQueue.ts`, a
// reducer with no clock and no network; this runs the EFFECTS of its transitions and feeds what happens to them (the timer fires, the answer comes) back in as
// events. The editor store hands over what it knows through `SaveHost` and keeps none of this itself.

/** What the runner asks of the editor it saves for. Each is read when it is needed, never kept: the document, the user and the timings change under it. */
export interface SaveHost {
  /** Somebody who cannot edit has nothing to save: the timers do nothing. */
  canEdit(): boolean
  /** The timings of the settings (`editor.autosaveMs`, `editor.saveRetryMs`, `editor.saveRetryMaxMs`). */
  config(): SaveConfig
  projectId(): string
  /** The working document and the revision it is based on, read at the moment a request is made. */
  document(): { arch: Architecture; rev: number }
  /** The server stored the document: its new revision. */
  stored(rev: number): void
}

export function createSaveRunner(host: SaveHost) {
  /** The state of the save path; replaced, never changed. */
  const queue = shallowRef<SaveQueue>(initialQueue())
  /** What the last failed save threw (not a conflict: that has words of its own). It is turned into text where it is shown, so that a language switch changes it. */
  const failure = shallowRef<unknown>(null)
  let timer: ReturnType<typeof setTimeout> | undefined
  let inflight: Promise<boolean> | null = null

  function dispatch(ev: SaveEvent) {
    const r = step(queue.value, ev, host.config())
    queue.value = r.state
    for (const fx of r.effects) run(fx)
  }

  function run(fx: SaveEffect) {
    if (fx.type === 'cancel') clearTimeout(timer)
    else if (fx.type === 'schedule') {
      clearTimeout(timer)
      timer = setTimeout(() => { if (host.canEdit()) dispatch({ type: fx.kind === 'autosave' ? 'autosaveDue' : 'retryDue' }) }, fx.ms)
    } else {
      const p = persist(fx.epoch)
      inflight = p
      void p.finally(() => { if (inflight === p) inflight = null })
    }
  }

  async function persist(epoch: number): Promise<boolean> {
    const { arch, rev } = host.document()
    const answer = await api.projects.saveDraft(host.projectId(), arch, rev).then((r) => ({ r }), (e: unknown) => ({ e }))
    if (epoch !== queue.value.epoch) return false // another project was opened meanwhile: this answer is not about the document on the screen
    if ('r' in answer) {
      host.stored(answer.r.rev)
      failure.value = null
      dispatch({ type: 'persistOk', epoch })
      return true
    }
    const e = answer.e
    const currentRev = e instanceof ApiError ? e.details?.currentRev : undefined
    dispatch({ type: 'persistFail', epoch, status: e instanceof ApiError ? e.status : null, currentRev: typeof currentRev === 'number' ? currentRev : null })
    // a conflict is told by its own text and button; keeping the server's sentence would put English into the tooltip of a Russian page
    failure.value = queue.value.status === 'conflict' ? null : e
    return false
  }

  /** The document was changed. */
  function touch() {
    if (host.canEdit()) dispatch({ type: 'edit' })
  }

  /** The network is back (the browser's `online`): a save that was waiting for it goes out now instead of after its pause. */
  function online() {
    if (host.canEdit()) dispatch({ type: 'online' })
  }

  /** Store the working copy now; resolves when everything that was edited so far is on the server. A refusal, a conflict or a lost connection: false. */
  async function flush(): Promise<boolean> {
    if (!host.projectId()) return true
    if (!host.canEdit()) return queue.value.status === 'saved'
    for (let round = 0; round < 3; round++) {
      const status = queue.value.status
      if (status === 'conflict') return false
      if (inflight) {
        if (!(await inflight)) return false
        continue
      }
      if (status === 'saved') return true
      dispatch({ type: 'flush' })
      if (!inflight) return queue.value.status === 'saved'
      if (!(await inflight)) return false
    }
    return queue.value.status === 'saved'
  }

  /** Another project is being opened (or this one reloaded): the timer, the request in flight and the failure of the one that was open are of no use any more. */
  function reset() {
    dispatch({ type: 'reset' })
    inflight = null // its request answers into nothing: its epoch is gone
    failure.value = null
  }

  return { queue, failure, dispatch, touch, online, flush, reset }
}
