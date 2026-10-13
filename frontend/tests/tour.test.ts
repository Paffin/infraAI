import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it } from 'vitest'
import en from '../src/i18n/en'
import ru from '../src/i18n/ru'
import { holeOf, placeCard, seenKey, stepKeys, TOURS, tourById, tourFor, visibleSteps, type Rect } from '../src/lib/tour'
import { safeHttpUrl, useApp } from '../src/stores/app'
import { TOUR_OFF_KEY, useTour } from '../src/stores/tour'

const VIEW = { w: 1600, h: 900 }
const CARD = { w: 360, h: 200 }
const fits = (p: { x: number; y: number }) => p.x >= 12 && p.y >= 12 && p.x + CARD.w <= VIEW.w - 12 && p.y + CARD.h <= VIEW.h - 12

describe('what the tours say', () => {
  it('have a title and a body in both languages for every step that has a card', () => {
    for (const def of TOURS) {
      for (const s of def.steps) {
        const k = stepKeys(def.id, s.id)
        for (const key of [k.title, k.body]) {
          expect((ru as Record<string, string>)[key], `ru ${key}`).toBeTruthy()
          expect((en as Record<string, string>)[key], `en ${key}`).toBeTruthy()
        }
      }
    }
  })

  it('belong to a route and name each step once', () => {
    expect(tourFor('projects')?.id).toBe('projects')
    expect(tourFor('editor')?.id).toBe('editor')
    expect(tourFor('admin')).toBeUndefined()
    expect(tourById('editor')?.route).toBe('editor')
    for (const def of TOURS) expect(new Set(def.steps.map((s) => s.id)).size).toBe(def.steps.length)
  })

  it('leave out the steps that point at something that is not on this screen', () => {
    const projects = tourById('projects')!
    const all = (_s: string) => true
    expect(visibleSteps(projects, all, true).map((s) => s.id)).toEqual(['new', 'list', 'admin', 'help'])
    // a user who is not an administrator has no entry to point at
    expect(visibleSteps(projects, all, false).map((s) => s.id)).toEqual(['new', 'list', 'help'])
    // a viewer has no "new project" button
    expect(visibleSteps(projects, (s) => !s.includes('new-project'), true).map((s) => s.id)).toEqual(['list', 'admin', 'help'])
    // steps without a target (the opening and the closing word) always stay
    const editor = tourById('editor')!
    expect(visibleSteps(editor, () => false, true).map((s) => s.id)).toEqual(['intro', 'done'])
  })

  it('keep the seen mark per user and per tour', () => {
    expect(seenKey('u1', 'editor')).not.toBe(seenKey('u2', 'editor'))
    expect(seenKey('u1', 'editor')).not.toBe(seenKey('u1', 'projects'))
    expect(seenKey(null, 'editor')).toContain('anon')
  })
})

describe('where the card goes', () => {
  const hole = (x: number, y: number, w: number, h: number): Rect => ({ x, y, w, h })

  it('puts the highlighted area with some air around it and inside the screen', () => {
    expect(holeOf(hole(100, 100, 50, 30), 8, VIEW)).toEqual({ x: 92, y: 92, w: 66, h: 46 })
    expect(holeOf(hole(2, 3, 50, 30), 8, VIEW)).toEqual({ x: 0, y: 0, w: 60, h: 41 })
    expect(holeOf(hole(1580, 880, 40, 40), 8, VIEW)).toEqual({ x: 1572, y: 872, w: 28, h: 28 })
  })

  it('prefers the side it was asked for', () => {
    const p = placeCard(hole(700, 100, 120, 40), CARD, VIEW, 'bottom')
    expect(p.placement).toBe('bottom')
    expect(p.y).toBe(100 + 40 + 14)
    expect(p.x).toBe(760 - CARD.w / 2)
    expect(fits(p)).toBe(true)
    expect(placeCard(hole(700, 600, 120, 40), CARD, VIEW, 'top').placement).toBe('top')
    expect(placeCard(hole(100, 400, 200, 100), CARD, VIEW, 'right').placement).toBe('right')
    expect(placeCard(hole(1300, 400, 200, 100), CARD, VIEW, 'left').placement).toBe('left')
  })

  it('turns to the opposite side when there is no room, then to the others', () => {
    expect(placeCard(hole(700, 800, 120, 40), CARD, VIEW, 'bottom').placement).toBe('top')
    expect(placeCard(hole(700, 20, 120, 40), CARD, VIEW, 'top').placement).toBe('bottom')
    expect(placeCard(hole(1500, 400, 90, 100), CARD, VIEW, 'right').placement).toBe('left')
    // a tall area that fills the width has no side or room above/below: the middle
    const wide = placeCard(hole(0, 0, 1600, 900), CARD, VIEW, 'bottom')
    expect(wide.placement).toBe('center')
    expect(fits(wide)).toBe(true)
  })

  it('never leaves the screen along the free axis', () => {
    for (const x of [0, 40, 800, 1500, 1590]) {
      const p = placeCard(hole(x, 100, 20, 20), CARD, VIEW, 'bottom')
      expect(fits(p), `x=${x}`).toBe(true)
    }
    for (const y of [0, 50, 450, 850]) {
      const p = placeCard(hole(300, y, 100, 30), CARD, VIEW, 'right')
      expect(fits(p), `y=${y}`).toBe(true)
    }
  })

  it('sits in the middle when there is nothing to point at', () => {
    const p = placeCard(null, CARD, VIEW, 'bottom')
    expect(p).toEqual({ x: (1600 - 360) / 2, y: (900 - 200) / 2, placement: 'center' })
    // and on a screen smaller than the card it still starts inside it
    expect(placeCard(null, CARD, { w: 300, h: 150 }, 'bottom').x).toBeGreaterThanOrEqual(12)
  })
})

describe('the documentation link', () => {
  it('is only ever an http(s) address', () => {
    expect(safeHttpUrl('https://docs.example.com/infra')).toBe('https://docs.example.com/infra')
    expect(safeHttpUrl(' http://wiki.local/ia ')).toBe('http://wiki.local/ia')
    expect(safeHttpUrl('javascript:alert(1)')).toBe('')
    expect(safeHttpUrl('data:text/html,<b>x</b>')).toBe('')
    expect(safeHttpUrl('//evil.example')).toBe('')
    expect(safeHttpUrl('')).toBe('')
    expect(safeHttpUrl(undefined)).toBe('')
    expect(safeHttpUrl(42)).toBe('')
  })
})

describe('the tour store', () => {
  beforeEach(() => {
    localStorage.clear()
    setActivePinia(createPinia())
    useApp().user = { id: 'u1', email: 'a@b.c', name: 'A', role: 'admin', mustChangePassword: false }
  })
  const editor = () => tourById('editor')!

  it('starts by itself once per user and screen', () => {
    const tour = useTour()
    expect(tour.canAutostart(editor())).toBe(true)
    tour.begin(editor(), editor().steps)
    expect(tour.active?.index).toBe(0)
    tour.finish()
    expect(tour.active).toBeNull()
    expect(tour.canAutostart(editor())).toBe(false)
    // another screen and another user still get theirs
    expect(tour.canAutostart(tourById('projects')!)).toBe(true)
    useApp().user = { id: 'u2', email: 'x@y.z', name: 'X', role: 'editor', mustChangePassword: false }
    expect(tour.canAutostart(editor())).toBe(true)
  })

  it('walks forward and back, and closing at the last step counts as done', () => {
    const tour = useTour()
    const steps = editor().steps
    tour.begin(editor(), steps)
    tour.next()
    tour.next()
    expect(tour.step?.id).toBe(steps[2].id)
    tour.prev()
    expect(tour.step?.id).toBe(steps[1].id)
    tour.go(99)
    expect(tour.isLast).toBe(true)
    tour.next()
    expect(tour.active).toBeNull()
    expect(tour.seen(editor())).toBe(true)
  })

  it('does not start without steps to show', () => {
    const tour = useTour()
    tour.begin(editor(), [])
    expect(tour.active).toBeNull()
  })

  it('can be switched off for this browser and back on', () => {
    const tour = useTour()
    tour.begin(editor(), editor().steps)
    tour.neverAgain()
    expect(tour.active).toBeNull()
    expect(localStorage.getItem(TOUR_OFF_KEY)).toBe('1')
    expect(tour.canAutostart(tourById('projects')!)).toBe(false)
    tour.forget()
    expect(tour.canAutostart(tourById('projects')!)).toBe(true)
  })

  it('does not start by itself when the administrator turned the tours off', () => {
    const tour = useTour()
    const app = useApp()
    expect(app.onboarding).toBe(true)
    app.boot = { app: { onboarding: false } } as never
    expect(app.onboarding).toBe(false)
    expect(tour.canAutostart(editor())).toBe(false)
  })
})

describe('the steps change while the screen fills in', () => {
  beforeEach(() => {
    localStorage.clear()
    setActivePinia(createPinia())
    useApp().user = { id: 'u1', email: 'a@b.c', name: 'A', role: 'admin', mustChangePassword: false }
  })

  it('keeps the step on screen when later steps appear or go', () => {
    const tour = useTour()
    const def = tourById('editor')!
    const without = def.steps.filter((s) => s.id !== 'whatif')
    tour.begin(def, without)
    tour.go(3) // score
    expect(tour.step?.id).toBe('score')
    tour.setSteps(def.steps) // the what-if bar has appeared
    expect(tour.count).toBe(def.steps.length)
    expect(tour.step?.id).toBe('score')
    tour.setSteps(def.steps.filter((s) => s.id !== 'score')) // the step on screen is gone: stay at the same place
    expect(tour.step?.id).toBe('checks')
    tour.setSteps([]) // nothing to show: leave the tour as it is
    expect(tour.count).toBeGreaterThan(0)
  })
})
