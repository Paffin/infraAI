import { describe, expect, it } from 'vitest'
import { clampPos, clampWidth, fracOf, parseFrac, placeFrac, room } from '@/lib/floating'

const area = { w: 1000, h: 600 }
const panel = { w: 300, h: 100 }

describe('where a floating panel sits', () => {
  it('can move in the area minus the panel and a margin on every side', () => {
    expect(room(area, panel, 10)).toEqual({ w: 680, h: 480 })
    expect(room({ w: 200, h: 50 }, panel, 10)).toEqual({ w: 0, h: 0 })
  })

  it('is kept inside the area', () => {
    expect(clampPos({ x: -50, y: -5 }, area, panel, 10)).toEqual({ x: 10, y: 10 })
    expect(clampPos({ x: 5000, y: 5000 }, area, panel, 10)).toEqual({ x: 690, y: 490 })
    expect(clampPos({ x: 100, y: 100 }, area, panel, 10)).toEqual({ x: 100, y: 100 })
    // a panel larger than the area sits at the margin instead of leaving through the other side
    expect(clampPos({ x: 300, y: 300 }, { w: 200, h: 50 }, panel, 10)).toEqual({ x: 10, y: 10 })
  })

  it('is stored as a fraction and put back at the same place of any other window', () => {
    const f = fracOf({ x: 350, y: 250 }, area, panel, 10)
    expect(f.fx).toBeCloseTo(0.5, 2)
    expect(f.fy).toBeCloseTo(0.5, 2)
    expect(placeFrac(f, area, panel, 10)).toEqual({ x: 350, y: 250 })
    // the window shrinks: the panel stays inside and in the same relative place
    const small = { w: 600, h: 400 }
    const p = placeFrac({ fx: 1, fy: 1 }, small, panel, 10)
    expect(p).toEqual({ x: 290, y: 290 })
    expect(clampPos(p, small, panel, 10)).toEqual(p)
  })

  it('reads a stored position and ignores what is not one', () => {
    expect(parseFrac('{"fx":0.25,"fy":1.5}')).toEqual({ fx: 0.25, fy: 1 })
    expect(parseFrac('{"fx":"a"}')).toBeNull()
    expect(parseFrac('not json')).toBeNull()
    expect(parseFrac(null)).toBeNull()
  })
})

describe('the width of a panel dragged by its edge', () => {
  it('stays between its limits and leaves the room next to it', () => {
    expect(clampWidth(100, 320, 720, 1600, 400)).toBe(320)
    expect(clampWidth(900, 320, 720, 1600, 400)).toBe(720)
    expect(clampWidth(500, 320, 720, 1600, 400)).toBe(500)
    // a narrow window: the neighbour keeps its room, the panel never goes below its own minimum
    expect(clampWidth(700, 320, 720, 900, 400)).toBe(500)
    expect(clampWidth(700, 320, 720, 600, 400)).toBe(320)
  })
})
