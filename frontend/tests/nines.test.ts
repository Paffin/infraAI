import { describe, expect, it } from 'vitest'
import { formatAvailability, targetFraction, yearlyDowntime } from '../src/lib/nines'
import { substitute } from '../src/i18n'

describe('availability as the nines it has', () => {
  // the same table the engine tests (`text::availability_shows_its_nines`): a number reads the same wherever it is worded
  it('writes the nines and one digit more', () => {
    expect([0.9995, 0.999, 0.99, 0.95, 0.5, 0].map(formatAvailability)).toEqual(['99.95%', '99.9%', '99%', '95%', '50%', '0%'])
    expect([0.9993412, 0.99999, 0.999996, 0.99984, 1].map(formatAvailability)).toEqual(['99.93%', '99.999%', '99.9996%', '99.98%', '100%'])
  })
  it('cuts, never rounds up to the next nine', () => {
    expect(formatAvailability(0.999999999)).toBe('99.9999999%')
    expect(formatAvailability(0.99949)).toBe('99.94%')
    expect(formatAvailability(0.99999999999)).not.toBe('100%')
  })
  it('keeps the value in range', () => {
    expect(formatAvailability(-1)).toBe('0%')
    expect(formatAvailability(7)).toBe('100%')
  })
  it('is a text format of the message templates, like the engine', () => {
    expect(substitute('{a|avail} of {t|avail}', { a: 0.99949, t: 0.9995 })).toBe('99.94% of 99.95%')
  })
  it('reads a target as a percentage or a fraction and counts the downtime it allows', () => {
    expect(targetFraction(99.95)).toBeCloseTo(0.9995, 12)
    expect(targetFraction(0.9995)).toBeCloseTo(0.9995, 12)
    expect(yearlyDowntime(0.999)).toBeCloseTo(31557.6, 3)
    expect(yearlyDowntime(0.9999) / 60).toBeCloseTo(52.596, 3)
    expect(yearlyDowntime(1)).toBe(0)
  })
})
