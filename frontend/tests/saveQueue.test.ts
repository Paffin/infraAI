import { describe, expect, it } from 'vitest'
import { classify, initialQueue, retryable, step, type SaveConfig, type SaveEffect, type SaveEvent, type SaveQueue } from '../src/lib/saveQueue'

// The save path as transitions of a reducer: an event goes in, the next state and the effects come out. There is no timer and no request here (the store runs
// the effects), so there is nothing to fake: the tests say what the editor must do, and the store is the one place that does it.

const cfg: SaveConfig = { autosaveMs: 900, retryMs: 3000, retryMaxMs: 30000 }

/** Feed events one after another; returns the last state and the effects of EACH step. */
function run(events: SaveEvent[], from: SaveQueue = initialQueue(), c: SaveConfig = cfg) {
  let state = from
  const effects: SaveEffect[][] = []
  for (const ev of events) {
    const r = step(state, ev, c)
    state = r.state
    effects.push(r.effects)
  }
  return { state, effects, last: effects[effects.length - 1] ?? [] }
}

const fail = (status: number | null, epoch = 0, currentRev?: number): SaveEvent => ({ type: 'persistFail', epoch, status, currentRev })
const ok: SaveEvent = { type: 'persistOk', epoch: 0 }

/** A state that is saving, as the store leaves it after an edit and the autosave timer. */
const saving = () => run([{ type: 'edit' }, { type: 'autosaveDue' }]).state

describe('the way to "saved"', () => {
  it('an edit makes the document dirty and starts the autosave timer', () => {
    const r = run([{ type: 'edit' }])
    expect(r.state.status).toBe('dirty')
    expect(r.last).toEqual([{ type: 'schedule', kind: 'autosave', ms: 900 }])
  })

  it('every edit restarts the autosave timer (one pause after the LAST edit)', () => {
    const r = run([{ type: 'edit' }, { type: 'edit' }, { type: 'edit' }])
    expect(r.effects.map((e) => e)).toEqual(Array(3).fill([{ type: 'schedule', kind: 'autosave', ms: 900 }]))
  })

  it('when the timer fires the document is sent, and the answer makes it saved', () => {
    const sending = run([{ type: 'edit' }, { type: 'autosaveDue' }])
    expect(sending.state.status).toBe('saving')
    expect(sending.last).toEqual([{ type: 'cancel' }, { type: 'persist', epoch: 0 }])
    const done = run([ok], sending.state)
    expect(done.state.status).toBe('saved')
    expect(done.last).toEqual([])
  })

  it('a timer that fires when nothing is dirty sends nothing', () => {
    expect(run([{ type: 'autosaveDue' }]).last).toEqual([])
    expect(run([{ type: 'autosaveDue' }]).state).toEqual(initialQueue())
  })

  it('an edit while a request is in flight does not make the answer "saved": the document is dirty again and the autosave starts', () => {
    const during = run([{ type: 'edit' }], saving())
    expect(during.state.status).toBe('saving')
    expect(during.state.edited).toBe(true)
    expect(during.last).toEqual([]) // nothing is scheduled while the request is out: its answer decides what comes next
    const answered = run([ok], during.state)
    expect(answered.state.status).toBe('dirty')
    expect(answered.state.edited).toBe(false)
    expect(answered.last).toEqual([{ type: 'schedule', kind: 'autosave', ms: 900 }])
  })

  it('an edit while a request is in flight that then fails is retried as a failure, not forgotten', () => {
    const r = run([{ type: 'edit' }, fail(0)], saving())
    expect(r.state.status).toBe('error')
    expect(r.state.edited).toBe(false)
    expect(r.last).toEqual([{ type: 'schedule', kind: 'retry', ms: 3000 }])
  })

  it('"flush" (Ctrl+S, leaving, making a version) sends what is dirty at once and cancels the pending timer', () => {
    const r = run([{ type: 'edit' }, { type: 'flush' }])
    expect(r.state.status).toBe('saving')
    expect(r.last).toEqual([{ type: 'cancel' }, { type: 'persist', epoch: 0 }])
  })

  it('"flush" with nothing to store, or with a request already out, starts nothing', () => {
    expect(run([{ type: 'flush' }]).last).toEqual([])
    expect(run([{ type: 'flush' }], saving()).last).toEqual([])
  })

  it('the server already holds the document ("synced"): saved, the timer is dropped', () => {
    const r = run([{ type: 'edit' }, { type: 'synced' }])
    expect(r.state.status).toBe('saved')
    expect(r.last).toEqual([{ type: 'cancel' }])
  })
})

describe('what a failed save does next', () => {
  it('no answer (status 0): "offline", retried after the first pause', () => {
    const r = run([fail(0)], saving())
    expect(r.state).toMatchObject({ status: 'error', failure: 'offline' })
    expect(r.last).toEqual([{ type: 'schedule', kind: 'retry', ms: 3000 }])
  })

  it('5xx: "server", retried', () => {
    for (const status of [500, 502, 503, 504]) {
      const r = run([fail(status)], saving())
      expect(r.state, String(status)).toMatchObject({ status: 'error', failure: 'server' })
      expect(r.last, String(status)).toEqual([{ type: 'schedule', kind: 'retry', ms: 3000 }])
    }
  })

  it('a failure that is not an answer at all (null) is a server failure and is retried', () => {
    const r = run([fail(null)], saving())
    expect(r.state.failure).toBe('server')
    expect(r.last).toEqual([{ type: 'schedule', kind: 'retry', ms: 3000 }])
  })

  it('409 is a conflict: no retry, no timer, the revision the server holds is kept', () => {
    const r = run([fail(409, 0, 7)], saving())
    expect(r.state).toMatchObject({ status: 'conflict', failure: null, serverRev: 7 })
    expect(r.last).toEqual([{ type: 'cancel' }])
    expect(run([fail(409)], saving()).state.serverRev).toBeNull()
  })

  it('401, 403, 404, 413, 422 and every other 4xx are refusals: "rejected", no retry', () => {
    for (const status of [400, 401, 403, 404, 413, 422, 429]) {
      const r = run([fail(status)], saving())
      expect(r.state, String(status)).toMatchObject({ status: 'error', failure: 'rejected' })
      expect(r.last, String(status)).toEqual([{ type: 'cancel' }])
    }
  })

  it('the pause doubles from retryMs up to retryMaxMs and stays there: 3000, 6000, 12000, 24000, 30000, 30000', () => {
    let s = saving()
    const waits: number[] = []
    for (let i = 0; i < 6; i++) {
      const failed = step(s, fail(0), cfg)
      const retry = failed.effects.find((e) => e.type === 'schedule')
      waits.push(retry && retry.type === 'schedule' ? retry.ms : -1)
      // the timer fires, the request goes out again
      s = step(failed.state, { type: 'retryDue' }, cfg).state
      expect(s.status).toBe('saving')
    }
    expect(waits).toEqual([3000, 6000, 12000, 24000, 30000, 30000])
  })

  it('the pauses are the settings, not 3000 and 30000', () => {
    const c: SaveConfig = { autosaveMs: 100, retryMs: 500, retryMaxMs: 1000 }
    let s = run([{ type: 'edit' }, { type: 'autosaveDue' }], initialQueue(), c).state
    const waits: number[] = []
    for (let i = 0; i < 3; i++) {
      const failed = step(s, fail(502), c)
      waits.push((failed.effects[0] as { ms: number }).ms)
      s = step(failed.state, { type: 'retryDue' }, c).state
    }
    expect(waits).toEqual([500, 1000, 1000])
  })

  it('a ceiling below the first pause is the ceiling', () => {
    const c: SaveConfig = { autosaveMs: 900, retryMs: 5000, retryMaxMs: 2000 }
    const r = run([{ type: 'edit' }, { type: 'autosaveDue' }, fail(0)], initialQueue(), c)
    expect(r.last).toEqual([{ type: 'schedule', kind: 'retry', ms: 2000 }])
  })

  it('a success resets the pauses', () => {
    let s = saving()
    s = step(step(step(s, fail(0), cfg).state, { type: 'retryDue' }, cfg).state, fail(0), cfg).state // two failures: the next pause would be 12000
    expect(s.delay).toBe(12000)
    s = step(step(s, { type: 'retryDue' }, cfg).state, ok, cfg).state
    expect(s).toMatchObject({ status: 'saved', failure: null, delay: 0 })
    const again = run([{ type: 'edit' }, { type: 'autosaveDue' }, fail(0)], s)
    expect(again.last).toEqual([{ type: 'schedule', kind: 'retry', ms: 3000 }])
  })

  it('the retry timer sends the document again; a retry timer that fires after something else happened sends nothing', () => {
    const failed = run([fail(0)], saving()).state
    expect(run([{ type: 'retryDue' }], failed).last).toEqual([{ type: 'cancel' }, { type: 'persist', epoch: 0 }])
    expect(run([{ type: 'retryDue' }], run([ok], saving()).state).last).toEqual([]) // saved already
    expect(run([{ type: 'retryDue' }], run([fail(422)], saving()).state).last).toEqual([]) // a refusal is not retried by a timer
  })

  it('an edit while waiting to retry goes back to the autosave timer (it replaces the retry timer) and the failure is remembered', () => {
    const failed = run([fail(0)], saving()).state
    const r = run([{ type: 'edit' }], failed)
    expect(r.state).toMatchObject({ status: 'dirty', failure: 'offline' })
    expect(r.last).toEqual([{ type: 'schedule', kind: 'autosave', ms: 900 }])
    // and the pause already earned is not forgotten: the next failure waits longer, not 3000 again
    const next = run([{ type: 'autosaveDue' }, fail(0)], r.state)
    expect(next.last).toEqual([{ type: 'schedule', kind: 'retry', ms: 6000 }])
  })

  it('an edit after a refusal tries again (the edit may be what the server wanted)', () => {
    const refused = run([fail(422)], saving()).state
    const r = run([{ type: 'edit' }, { type: 'autosaveDue' }], refused)
    expect(r.state.status).toBe('saving')
    expect(r.last).toEqual([{ type: 'cancel' }, { type: 'persist', epoch: 0 }])
  })

  it('in a conflict edits are ignored: nothing is sent, nothing is scheduled', () => {
    const conflict = run([fail(409, 0, 3)], saving()).state
    for (const ev of [{ type: 'edit' }, { type: 'autosaveDue' }, { type: 'retryDue' }, { type: 'flush' }, { type: 'online' }] as SaveEvent[]) {
      const r = step(conflict, ev, cfg)
      expect(r.state, ev.type).toEqual(conflict)
      expect(r.effects, ev.type).toEqual([])
    }
  })
})

describe('the network coming back', () => {
  it('"online" after an offline failure sends at once, cancels the pending retry and starts the pauses over', () => {
    const waiting = run([fail(0)], run([{ type: 'edit' }, { type: 'autosaveDue' }, fail(0), { type: 'retryDue' }]).state).state
    expect(waiting.delay).toBe(12000)
    const r = run([{ type: 'online' }], waiting)
    expect(r.state).toMatchObject({ status: 'saving', delay: 0 })
    expect(r.last).toEqual([{ type: 'cancel' }, { type: 'persist', epoch: 0 }])
  })

  it('"online" while an edit made during the outage is waiting for its autosave sends it at once', () => {
    const r = run([{ type: 'edit' }, { type: 'online' }], run([fail(0)], saving()).state)
    expect(r.state.status).toBe('saving')
    expect(r.last).toEqual([{ type: 'cancel' }, { type: 'persist', epoch: 0 }])
  })

  it('"online" after a 5xx also retries at once', () => {
    expect(run([{ type: 'online' }], run([fail(503)], saving()).state).last).toEqual([{ type: 'cancel' }, { type: 'persist', epoch: 0 }])
  })

  it('"online" does nothing when nothing failed or the failure is not about the network', () => {
    for (const from of [initialQueue(), run([{ type: 'edit' }]).state, saving(), run([fail(409, 0, 2)], saving()).state, run([fail(422)], saving()).state]) {
      const r = step(from, { type: 'online' }, cfg)
      expect(r.effects, from.status + '/' + from.failure).toEqual([])
      expect(r.state, from.status + '/' + from.failure).toEqual(from)
    }
  })
})

describe('a document that is gone', () => {
  it('"reset" starts over with a new epoch and drops the timer', () => {
    const dirty = run([{ type: 'edit' }, { type: 'autosaveDue' }, fail(0)]).state
    const r = run([{ type: 'reset' }], dirty)
    expect(r.state).toEqual(initialQueue(1))
    expect(r.state.delay).toBe(0)
    expect(r.last).toEqual([{ type: 'cancel' }])
  })

  it('an answer that carries an older epoch is about another document and changes nothing', () => {
    const inFlight = saving() // epoch 0, request out
    const reopened = step(inFlight, { type: 'reset' }, cfg).state
    const editing = step(reopened, { type: 'edit' }, cfg).state
    const late = step(editing, { type: 'persistOk', epoch: 0 }, cfg)
    expect(late.state).toEqual(editing)
    expect(late.effects).toEqual([])
    const lateFail = step(step(editing, { type: 'autosaveDue' }, cfg).state, fail(409, 0, 9), cfg)
    expect(lateFail.state.status).toBe('saving')
    expect(lateFail.state.serverRev).toBeNull()
  })

  it('an answer when no request is out is ignored', () => {
    expect(step(initialQueue(), ok, cfg)).toEqual({ state: initialQueue(), effects: [] })
    expect(step(initialQueue(), fail(500), cfg)).toEqual({ state: initialQueue(), effects: [] })
  })

  it('the request carries the epoch it was started in', () => {
    const reopened = step(step(initialQueue(), { type: 'reset' }, cfg).state, { type: 'reset' }, cfg).state
    const r = run([{ type: 'edit' }, { type: 'autosaveDue' }], reopened)
    expect(r.last).toEqual([{ type: 'cancel' }, { type: 'persist', epoch: 2 }])
  })
})

describe('one timer at most', () => {
  it('whatever starts a request cancels the timer first; whatever starts a timer replaces the pending one', () => {
    const states: SaveQueue[] = [
      run([{ type: 'edit' }]).state, // dirty, autosave pending
      run([fail(0)], saving()).state, // error, retry pending
    ]
    for (const from of states) {
      for (const ev of [{ type: 'flush' }, { type: 'online' }] as SaveEvent[]) {
        const fx = step(from, ev, cfg).effects
        if (fx.some((e) => e.type === 'persist')) expect(fx[0], `${from.status} ${ev.type}`).toEqual({ type: 'cancel' })
      }
    }
    // a retry after a failure is the only timer: it is one `schedule` effect, which the store implements as clear + set
    const failed = step(saving(), fail(0), cfg)
    expect(failed.effects.filter((e) => e.type === 'schedule')).toHaveLength(1)
  })
})

describe('the classification', () => {
  it('maps a status to what the next step is', () => {
    expect(classify(0)).toBe('offline')
    expect(classify(null)).toBe('server')
    expect(classify(500)).toBe('server')
    expect(classify(599)).toBe('server')
    expect(classify(400)).toBe('rejected')
    expect(classify(401)).toBe('rejected')
    expect(classify(499)).toBe('rejected')
    expect(retryable('offline')).toBe(true)
    expect(retryable('server')).toBe(true)
    expect(retryable('rejected')).toBe(false)
    expect(retryable(null)).toBe(false)
  })

  it('does not change the state it is given', () => {
    const before = saving()
    const copy = JSON.parse(JSON.stringify(before))
    step(before, fail(0), cfg)
    step(before, { type: 'edit' }, cfg)
    step(before, ok, cfg)
    expect(before).toEqual(copy)
  })
})
