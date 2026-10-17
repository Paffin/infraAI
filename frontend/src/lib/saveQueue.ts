// The save path of the editor as a reducer: `step(state, event, cfg)` returns the next state and the EFFECTS that follow from it. There is no timer, no request
// and no clock in here; the store (`stores/editor.ts`) is the one place that interprets the effects (starts a timer, sends the draft) and feeds what happens back
// as events. That is why the rules below are tested as plain transitions, without a fake server or fake timers:
//
//   edit while saving   → the answer to the request in flight does not mean "saved": the document moved on
//   answer 409          → conflict; nothing is retried, the person decides
//   answer 0 or 5xx     → the connection or the server: retried with a pause that doubles up to a ceiling
//   any other 4xx       → the server read the request and refused it: retrying the same request cannot help
//   `online`            → the network came back: the pause is not waited out
//
// Every `schedule` effect REPLACES the timer that was pending (there is at most one), so a retry can never be followed by a second, earlier timer that
// skips the pause.

export type SaveStatus = 'saved' | 'dirty' | 'saving' | 'conflict' | 'error'

/**
 * Why the last attempt failed, as far as the next step is concerned: `offline` (no answer at all), `server` (an answer of 5xx, or a failure that is neither),
 * `rejected` (a 4xx other than 409: the server will refuse it again). It stays until a save succeeds, so that an edit made during an outage does not make the
 * message flicker; `null` when nothing failed.
 */
export type SaveFailure = 'offline' | 'server' | 'rejected'

export interface SaveQueue {
  status: SaveStatus
  failure: SaveFailure | null
  /** The pause before the next retry, ms; 0 = the next one is the first (`cfg.retryMs`). */
  delay: number
  /** Which document the state is about: `reset` starts a new one, and an answer that carries an older number is about a document that is gone. */
  epoch: number
  /** The document was edited while a request was in flight: that request's answer is not "saved". */
  edited: boolean
  /** The revision the server holds, when it said so with a 409; otherwise null. */
  serverRev: number | null
}

/** The timings (`editor.autosaveMs`, `editor.saveRetryMs`, `editor.saveRetryMaxMs`). */
export interface SaveConfig { autosaveMs: number; retryMs: number; retryMaxMs: number }

export type SaveEvent =
  /** The document was changed. */
  | { type: 'edit' }
  /** The autosave timer fired. */
  | { type: 'autosaveDue' }
  /** The retry timer fired. */
  | { type: 'retryDue' }
  /** Somebody wants it stored now (Ctrl+S, leaving the page, making a version): also a manual retry. */
  | { type: 'flush' }
  /** The browser says the network is back. */
  | { type: 'online' }
  /** The server already holds this document (an applied AI proposal was stored by the server itself). */
  | { type: 'synced' }
  | { type: 'persistOk'; epoch: number }
  /** `status`: the HTTP status of the answer, 0 for no answer at all, null for a failure that is not an answer. `currentRev`: what a 409 says the server holds. */
  | { type: 'persistFail'; epoch: number; status: number | null; currentRev?: number | null }
  /** Another project is being opened (or the same one reloaded): everything starts over. */
  | { type: 'reset' }

export type SaveEffect =
  /** Start a timer that, when it fires, is `autosaveDue` or `retryDue`; replaces the pending timer. */
  | { type: 'schedule'; kind: 'autosave' | 'retry'; ms: number }
  /** Drop the pending timer. */
  | { type: 'cancel' }
  /** Send the document as it is now; report the result with the same `epoch`. */
  | { type: 'persist'; epoch: number }

export interface Step { state: SaveQueue; effects: SaveEffect[] }

export const initialQueue = (epoch = 0): SaveQueue => ({ status: 'saved', failure: null, delay: 0, epoch, edited: false, serverRev: null })

/** What a failed request means for the next step. */
export function classify(status: number | null): SaveFailure {
  if (status === 0) return 'offline'
  if (status === null || status >= 500) return 'server'
  return 'rejected'
}

/** A failure that a later attempt can cure by itself: the network or the server coming back. */
export const retryable = (f: SaveFailure | null): boolean => f === 'offline' || f === 'server'

const only = (state: SaveQueue): Step => ({ state, effects: [] })

export function step(s: SaveQueue, ev: SaveEvent, cfg: SaveConfig): Step {
  switch (ev.type) {
    case 'edit':
      if (s.status === 'conflict') return only(s) // the person has not decided yet; their edits are kept on the screen, not sent
      if (s.status === 'saving') return only({ ...s, edited: true })
      return { state: { ...s, status: 'dirty' }, effects: [{ type: 'schedule', kind: 'autosave', ms: cfg.autosaveMs }] }

    case 'autosaveDue':
      return s.status === 'dirty' ? send(s) : only(s)

    case 'retryDue':
      return s.status === 'error' && retryable(s.failure) ? send(s) : only(s)

    case 'flush':
      return s.status === 'dirty' || s.status === 'error' ? send(s) : only(s)

    case 'online':
      // the pause was for a network that was gone; it is not waited out, and the next failure starts the pauses again
      if (retryable(s.failure) && (s.status === 'error' || s.status === 'dirty')) return send({ ...s, delay: 0 })
      return only(s)

    case 'synced':
      return s.status === 'saving' ? only(s) : { state: { ...s, status: 'saved', failure: null, delay: 0, edited: false, serverRev: null }, effects: [{ type: 'cancel' }] }

    case 'persistOk':
      if (ev.epoch !== s.epoch || s.status !== 'saving') return only(s)
      if (s.edited) return { state: { ...s, status: 'dirty', failure: null, delay: 0, edited: false }, effects: [{ type: 'schedule', kind: 'autosave', ms: cfg.autosaveMs }] }
      return only({ ...s, status: 'saved', failure: null, delay: 0, edited: false })

    case 'persistFail': {
      if (ev.epoch !== s.epoch || s.status !== 'saving') return only(s)
      if (ev.status === 409) return { state: { ...s, status: 'conflict', failure: null, edited: false, serverRev: ev.currentRev ?? null }, effects: [{ type: 'cancel' }] }
      const failure = classify(ev.status)
      if (!retryable(failure)) return { state: { ...s, status: 'error', failure, edited: false }, effects: [{ type: 'cancel' }] }
      const wait = Math.min(s.delay || cfg.retryMs, cfg.retryMaxMs)
      return {
        state: { ...s, status: 'error', failure, edited: false, delay: Math.min(wait * 2, cfg.retryMaxMs) },
        effects: [{ type: 'schedule', kind: 'retry', ms: wait }],
      }
    }

    case 'reset':
      return { state: initialQueue(s.epoch + 1), effects: [{ type: 'cancel' }] }
  }
}

/** Start a request now: whatever timer was pending is of no use any more. */
function send(s: SaveQueue): Step {
  return { state: { ...s, status: 'saving', edited: false }, effects: [{ type: 'cancel' }, { type: 'persist', epoch: s.epoch }] }
}
