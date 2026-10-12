/** Colour band of a 0–100 resilience score. */
export function scoreTone(v: number | null | undefined): 'ok' | 'risk' | 'crit' | 'none' {
  if (v == null) return 'none'
  return v >= 85 ? 'ok' : v >= 60 ? 'risk' : 'crit'
}
