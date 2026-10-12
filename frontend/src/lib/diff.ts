import type { ArchDiff, Change } from '@/api/types'

export interface DiffLine { sign: '+' | '-' | '~'; text: string; group: 'requirements' | 'zones' | 'nodes' | 'edges' }

const short = (v: unknown): string => {
  if (v === null || v === undefined) return '∅'
  if (typeof v === 'string') return v.length > 28 ? v.slice(0, 27) + '…' : v
  if (typeof v === 'object') return JSON.stringify(v).slice(0, 28)
  return String(v)
}

const changeText = (c: Change) => `${c.path}: ${short(c.from)} → ${short(c.to)}`

/** Flatten an architecture diff into reviewable lines (added / removed / changed). */
export function diffLines(d: ArchDiff, perEntity = 2): DiffLine[] {
  const out: DiffLine[] = []
  for (const c of d.requirements) out.push({ sign: '~', text: changeText(c), group: 'requirements' })
  for (const group of ['zones', 'nodes', 'edges'] as const) {
    const e = d[group]
    for (const x of e.added) out.push({ sign: '+', text: x.name || x.id, group })
    for (const x of e.removed) out.push({ sign: '-', text: x.name || x.id, group })
    for (const x of e.changed) {
      const shown = x.changes.slice(0, perEntity).map(changeText).join('; ')
      const more = x.changes.length > perEntity ? ` (+${x.changes.length - perEntity})` : ''
      out.push({ sign: '~', text: `${x.name || x.id}: ${shown}${more}`, group })
    }
  }
  return out
}

export function diffIsEmpty(d: ArchDiff): boolean {
  return !d.requirements.length && (['zones', 'nodes', 'edges'] as const).every((g) => !d[g].added.length && !d[g].removed.length && !d[g].changed.length)
}
