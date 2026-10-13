import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { DURATIONS, ease, lerp, parseCssTime, parsePref, resolveMode, scaleFactor, stagger, tween } from '../src/lib/motion'

describe('which animations play', () => {
  it('follows the user, then the administrator; "system" asks the operating system', () => {
    expect(resolveMode(null, 'full', true)).toBe('full')
    expect(resolveMode(null, 'system', true)).toBe('reduced')
    expect(resolveMode(null, 'system', false)).toBe('full')
    expect(resolveMode('off', 'full', false)).toBe('off')
    expect(resolveMode('full', 'off', true)).toBe('full')
    expect(resolveMode('system', 'off', true)).toBe('reduced')
    expect(resolveMode('reduced', 'system', false)).toBe('reduced')
  })

  it('accepts only known preferences', () => {
    for (const p of ['system', 'full', 'reduced', 'off']) expect(parsePref(p)).toBe(p)
    expect(parsePref('fast')).toBeNull()
    expect(parsePref(null)).toBeNull()
    expect(parsePref(3)).toBeNull()
  })

  it('turns the percent of the setting into a multiplier within its limits', () => {
    expect(scaleFactor(100)).toBe(1)
    expect(scaleFactor(50)).toBe(0.5)
    expect(scaleFactor(1000)).toBe(3)
    expect(scaleFactor(1)).toBe(0.25)
    expect(scaleFactor('150')).toBe(1.5)
    expect(scaleFactor('fast')).toBe(1)
    expect(scaleFactor(undefined)).toBe(1)
  })
})

describe('arithmetic of motion', () => {
  it('eases from 0 to 1 and stays inside', () => {
    for (const f of [ease.linear, ease.out, ease.inOut]) {
      expect(f(0)).toBeCloseTo(0)
      expect(f(1)).toBeCloseTo(1)
      for (let i = 1; i < 10; i++) expect(f(i / 10)).toBeGreaterThan(0)
    }
    expect(ease.out(0.5)).toBeGreaterThan(0.5) // fast start
    expect(ease.inOut(0.25)).toBeLessThan(0.25) // slow start
    expect(lerp(10, 20, 0.5)).toBe(15)
  })

  it('staggers a list without letting a long one crawl', () => {
    expect(stagger(0, 36, 300)).toBe(0)
    expect(stagger(3, 36, 300)).toBe(108)
    expect(stagger(100, 36, 300)).toBe(300)
    expect(stagger(-2, 36, 300)).toBe(0)
  })

  it('reads CSS times the way the browser reports them', () => {
    expect(parseCssTime('0.14s')).toBe(140)
    expect(parseCssTime('140ms')).toBe(140)
    expect(parseCssTime('0.14s, 0.2s')).toBe(200)
    expect(parseCssTime('0s')).toBe(0)
    expect(parseCssTime('')).toBe(0)
    expect(parseCssTime('calc(140ms * 1)')).toBe(0)
    expect(parseCssTime(undefined)).toBe(0)
  })
})

describe('tween', () => {
  /** A clock the test turns by hand instead of waiting for frames. */
  function clock() {
    let cb: FrameRequestCallback | null = null
    let handle = 0
    return {
      raf: (f: FrameRequestCallback) => { cb = f; return ++handle },
      cancel: () => { cb = null },
      tick: (now: number) => { const f = cb; cb = null; f?.(now) },
      pending: () => cb !== null,
    }
  }

  it('delivers the end value at once when there is no time', () => {
    const seen: number[] = []
    tween(0, 10, 0, (v) => seen.push(v))
    expect(seen).toEqual([10])
  })

  it('moves frame by frame and ends exactly on the target', () => {
    const c = clock()
    const seen: number[] = []
    tween(0, 100, 200, (v) => seen.push(v), ease.linear, c.raf, c.cancel)
    c.tick(1000) // the first frame fixes the start
    c.tick(1100)
    c.tick(1200)
    expect(seen[0]).toBe(0)
    expect(seen[1]).toBeCloseTo(50)
    expect(seen[seen.length - 1]).toBe(100)
    expect(c.pending()).toBe(false)
  })

  it('stops when cancelled', () => {
    const c = clock()
    const seen: number[] = []
    const t = tween(0, 100, 200, (v) => seen.push(v), ease.linear, c.raf, c.cancel)
    c.tick(1000)
    t.cancel()
    c.tick(1100)
    expect(seen).toEqual([0])
  })
})

describe('the duration tokens', () => {
  const css = readFileSync(join(process.cwd(), 'src', 'styles', 'tokens.css'), 'utf8')

  it('are the same in the stylesheet and in the code that animates by itself', () => {
    for (const [name, ms] of Object.entries(DURATIONS)) {
      const m = new RegExp(`--dur-${name}:\\s*calc\\((\\d+)ms \\* var\\(--motion-scale\\)\\)`).exec(css)
      expect(m, `--dur-${name} in tokens.css`).not.toBeNull()
      expect(Number(m![1]), name).toBe(ms)
    }
  })

  it('are ordered from the shortest to the longest', () => {
    const v = Object.values(DURATIONS)
    expect([...v].sort((a, b) => a - b)).toEqual(v)
  })
})
