// The failure as a written account: what it is, what it costs, and what happens step by step: Markdown that goes into a ticket, an ADR or a
// post-mortem. It says what the card says (the same figures and the same sentences), in the language of the interface.

import type { ServiceState, SimResult } from '@/api/types'
import { t } from '@/i18n'
import { fmtDuration, fmtNumber } from './format'
import type { Step } from './playback'
import { eventText } from './sim'
import type { Impact, Hit } from './simImpact'
import type { Tile } from './simTiles'

export interface StoryInput {
  title: string
  res: Pick<SimResult, 'events' | 'rpoAssumed'>
  /** the service state once everything has settled */
  state: ServiceState
  steps: Step[]
  tiles: Tile[]
  impact: Impact
  /** the share of the requests served once everything has settled, and in normal operation (when the engine told) */
  served: number | null
  servedBefore: number | null
  /** the declared peak load, to say the traffic in requests per second */
  rps: number | null
  /** the day the account is written, `2026-10-13` */
  date: string
}

const pct = (v: number) => `${Math.round(v * 100)}%`
const atText = (s: { at: number; kind: string }) => (s.kind === 'before' ? t('play.before') : s.at <= 0 ? t('play.now') : '+' + fmtDuration(s.at))
const names = (hits: Hit[]) => hits.map((h) => `«${h.name}»` + (h.zone ? ` (${h.zone})` : '')).join(', ')

export function storyMarkdown(i: StoryInput): string {
  const out: string[] = [`# ${i.title}`, '']
  out.push(`**${t('story.md.result')}:** ${t('sim.result.' + i.state)}.`, '')
  for (const tile of i.tiles) out.push(`- ${tile.label}: **${tile.value}**${tile.hint ? ` (${tile.hint})` : ''}`)
  out.push('')

  if (i.served !== null) {
    const normal = i.servedBefore !== null && i.servedBefore !== i.served ? ` ${t('traffic.was', { pct: pct(i.servedBefore) })}` : ''
    const rate = i.rps ? `, ${fmtNumber(Math.round(i.served * i.rps))} ${t('traffic.of', { total: fmtNumber(i.rps) })}` : ''
    out.push(`**${t('traffic.title')}:** ${pct(i.served)} ${t('traffic.served')}${normal ? ` (${normal.trim()})` : ''}${rate}.`, '')
  }

  out.push(`## ${t('sim.details')}`, '')
  if (i.steps.length) {
    for (const s of i.steps) {
      out.push(`### ${atText(s)}${s.served !== null ? ` · ${pct(s.served)}` : ''}`)
      if (s.kind === 'before') out.push(`- **${t('play.before.step')}.** ${t('play.before.text')}`)
      else if (s.events.length) for (const e of s.events) out.push(`- ${eventText(e)}`)
      else out.push(`- ${t('play.settled')}`)
      out.push('')
    }
  } else {
    for (const e of i.res.events) out.push(`- ${eventText(e)}`)
    if (!i.res.events.length) out.push(`- ${t('sim.noevents')}`)
    out.push('')
  }
  if (i.res.rpoAssumed) out.push(`_${t('sim.assumed')}_`, '')

  const tiers: [string, Hit[]][] = [[t('simimpact.out'), i.impact.out], [t('simimpact.hurt'), i.impact.hurt], [t('simimpact.took'), i.impact.took]]
  if (tiers.some(([, hits]) => hits.length)) {
    out.push(`## ${t('simimpact.title')}`, '')
    for (const [label, hits] of tiers) if (hits.length) out.push(`- ${label}: ${names(hits)}`)
    out.push('')
  }
  out.push('---', `_${t('story.md.footer', { date: i.date })}_`, '')
  return out.join('\n')
}
