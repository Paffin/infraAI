/**
 * An availability (0…1) as a percentage with the nines it has and one more digit: `99.95%`, `99.9%`, `99.999%`, `95%`. The digits are
 * cut, not rounded: 0.99996 is never shown as 100%. The same rule as the engine's `{x|avail}` text format, so a number reads the same in a
 * finding the engine worded and in a panel the editor draws.
 */
export function formatAvailability(value: number): string {
  const a = Math.min(1, Math.max(0, value))
  if (a >= 1) return '100%'
  const nines = Math.max(0, Math.floor(-Math.log10(1 - a)))
  const decimals = Math.min(9, Math.max(0, nines - 1))
  const scale = 10 ** decimals
  const cut = Math.floor(a * 100 * scale + 1e-7) / scale
  let text = cut.toFixed(decimals)
  if (text.includes('.')) text = text.replace(/0+$/, '').replace(/\.$/, '')
  return `${text}%`
}

/** A target as the requirement holds it: a percentage (`99.95`); a value up to 1 is taken as a fraction (`0.9995`). */
export function targetFraction(slo: number): number {
  return slo > 1 ? slo / 100 : slo
}

/** Seconds of downtime a year the availability allows. */
export function yearlyDowntime(availability: number, daysPerYear = 365.25): number {
  return Math.max(0, (1 - Math.min(1, Math.max(0, availability))) * daysPerYear * 86400)
}
