/**
 * What a screen reader is told when the engine's verdict changes after an edit. The page shows it (the dots on the cards, the badge on the rail, the list of checks);
 * somebody who does not see the page learns nothing unless it is said. `describeChange` says what changed in words; `announce` hands it to the one live region of the
 * editor (`components/LiveRegion.vue`).
 *
 * Judged by fingerprints, not by counts: one critical finding fixed and another introduced by the same edit leave the count where it was, and that is the very edit
 * somebody has to be told about. A move of a card changes no finding: silence.
 */
import { nextTick, ref } from 'vue'
import type { Finding, Report, Rule } from '@/api/types'
import { t } from '@/i18n'
import { findingTitle, statusRank } from './findings'

/** How many rules are named in the message: a list that is read to the end is a list nobody listens to. */
const NAMED = 2

/** The findings a message is worth: open ones (not accepted), critical or a risk. A finding with no data is counted by its rule (below), it is not a verdict. */
const loud = (f: Finding) => !f.accepted && (f.status === 'critical' || f.status === 'risk')

export interface ReportDelta {
  /** New at this level, or worse than before (a risk that became critical), the worst first, the order of the report inside a level. */
  introduced: Finding[]
  /** Open before, gone now. */
  resolved: Finding[]
  /** The change in the number of checks that cannot say for lack of data (positive: more of them). */
  noData: number
}

export function diffReports(prev: Report, next: Report): ReportDelta {
  const before = new Map(prev.findings.filter(loud).map((f) => [f.id, f]))
  const after = new Map(next.findings.filter(loud).map((f) => [f.id, f]))
  const introduced = [...after.values()].filter((f) => {
    const was = before.get(f.id)
    return !was || statusRank[f.status] < statusRank[was.status]
  })
  const resolved = [...before.values()].filter((f) => !after.has(f.id))
  introduced.sort((a, b) => statusRank[a.status] - statusRank[b.status])
  return { introduced, resolved, noData: next.counts.noData - prev.counts.noData }
}

/** The ids of rules in the order of first appearance, without repeats: three subjects of one rule are one thing to say. */
function distinctRules(list: readonly Finding[]): string[] {
  return [...new Set(list.map((f) => f.ruleId))]
}

/**
 * The message for the change from `prev` to `next`, in the language of the page; `null` when nothing a person has to be told about changed (the same findings
 * are open, and the same number of checks lack data).
 */
export function describeChange(prev: Report, next: Report, rules: { get(id: string): Rule | undefined }): string | null {
  const d = diffReports(prev, next)
  const parts: string[] = []
  const crit = d.introduced.filter((f) => f.status === 'critical').length
  const risk = d.introduced.length - crit
  if (crit) parts.push(t('announce.crit', { n: crit }))
  if (risk) parts.push(t('announce.risk', { n: risk }))
  if (d.resolved.length) parts.push(t('announce.resolved', { n: d.resolved.length }))
  if (d.noData > 0) parts.push(t('announce.nodata.more', { n: d.noData }))
  if (d.noData < 0) parts.push(t('announce.nodata.less', { n: -d.noData }))
  if (!parts.length) return null

  let text = parts.join(', ')
  text = text.charAt(0).toUpperCase() + text.slice(1) + '.'
  // which rules the new findings are of (the worst first)
  const ruleIds = distinctRules(d.introduced)
  if (ruleIds.length) {
    const first = ruleIds.slice(0, NAMED).map((id) => {
      const f = d.introduced.find((x) => x.ruleId === id)!
      return t('announce.name', { name: findingTitle(rules.get(id), f) })
    })
    const more = ruleIds.length - first.length
    text += ' ' + t('announce.names', { names: first.join(', ') + (more > 0 ? t('announce.names.more', { n: more }) : '') }) + '.'
  }
  return text
}

// ---------------------------------------------------------------------------------------------------------------------- the live region
/** The text of the live region of the editor. */
export const liveMessage = ref('')

let seq = 0
let timer: ReturnType<typeof setTimeout> | undefined

/**
 * Say it. A message equal to the one that is there is read again only if the text is taken away first and put back a moment later (a screen reader reads a change,
 * not a rewrite of the same words), so the region is cleared first in that case.
 */
export function announce(text: string) {
  const mine = ++seq
  clearTimeout(timer)
  if (text === liveMessage.value) {
    liveMessage.value = ''
    // the cleared region has to reach the page before the text does, or the two changes are one
    void nextTick(() => { timer = setTimeout(() => { if (mine === seq) liveMessage.value = text }, 60) })
  } else liveMessage.value = text
}

/** Silence (the editor is left: nothing of it is read later). */
export function clearAnnouncement() {
  seq++
  clearTimeout(timer)
  liveMessage.value = ''
}
