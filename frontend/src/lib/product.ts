/** Product facts of a node for the editor: which release tables judge its versions and what they say. A mirror of the
 * engine's `version_refs` (checks.rs). */
import type { Cycle, KindDef, Lifecycle, Props, Scalar, VariantDef, VersionSet } from '@/api/types'
import { lt } from '@/i18n'
import { effectiveProps, tableIdOf, variantOf, type CatalogIndex } from './catalog'
import { lifecycleOf, recommended, verdictOf, ageDays, type Verdict, type VersionIndex } from './lifecycle'

export interface VersionRef {
  /** The prop that holds the version (`version`, `osVersion`). */
  prop: string
  set: VersionSet
  /** What is being judged, for messages (`PostgreSQL`, `Ubuntu`). */
  product: string
}

/** Every version a node can declare that a release table judges: its own `version` and the props marked `lifecycle`. */
export function versionRefs(ix: CatalogIndex, vx: VersionIndex | null, kindId: string, declared: Props): VersionRef[] {
  const out: VersionRef[] = []
  const kind = ix.kinds.get(kindId)
  if (!kind || !vx) return out
  const variant = variantOf(kind, declared.variant)
  const own = tableIdOf(kind, variant)
  const set = own ? vx.tables.get(own) : undefined
  if (set && kind.props.some((p) => p.key === 'version')) out.push({ prop: 'version', set, product: set.name || lt(kind.name) })
  const eff = effectiveProps(ix, kindId, declared)
  for (const p of kind.props) {
    if (!p.lifecycle) continue
    const table = p.lifecycle.startsWith('@') ? eff[p.lifecycle.slice(1)] : p.lifecycle
    const s = typeof table === 'string' ? vx.tables.get(table) : undefined
    if (s) out.push({ prop: p.key, set: s, product: s.name || s.id })
  }
  return out
}

export interface VersionView {
  lifecycle: Lifecycle | null
  verdict: Verdict
  /** The line the engine would recommend today. */
  recommended: Cycle | null
  /** The table is older than the engine trusts. */
  stale: boolean
  ageDays: number | null
}

export function viewVersion(vx: VersionIndex, set: VersionSet, version: Scalar | undefined | null): VersionView {
  const text = version === undefined || version === null ? '' : String(version)
  const lifecycle = text ? lifecycleOf(set, text, vx.today) : null
  const age = ageDays(set, vx.today)
  return {
    lifecycle,
    verdict: text ? verdictOf(lifecycle, vx.settings.warnDays) : 'unknown',
    recommended: recommended(set, vx.today, vx.settings.preferLts, vx.settings.matureDays),
    stale: age !== null && age > vx.settings.staleDays,
    ageDays: age,
  }
}

/** The verdict of each cycle of a table today (for the list of lines to pick from). */
export function cycleVerdict(vx: VersionIndex, set: VersionSet, c: Cycle): Verdict {
  return verdictOf(lifecycleOf(set, c.cycle, vx.today), vx.settings.warnDays)
}

/** Name of a node's variant for subtitles and badges ('' for a kind without variants). */
export function variantName(kind: KindDef | undefined, variant: VariantDef | undefined): string {
  return kind?.variants?.length && variant ? lt(variant.name) : ''
}

/** Is the node run by a provider (a managed service)? */
export function isManaged(variant: VariantDef | undefined, kind: KindDef | undefined): boolean {
  return !!variant?.addTraits?.includes('managed') || !!kind?.traits.includes('managed')
}
