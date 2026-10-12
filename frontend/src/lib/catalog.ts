import type { ArchNode, Catalog, EdgeTypeDef, KindDef, LText, PropDef, Props, Scalar, ZoneTypeDef } from '@/api/types'
import { lt } from '@/i18n'

export interface CatalogIndex {
  catalog: Catalog
  kinds: Map<string, KindDef>
  edgeTypes: Map<string, EdgeTypeDef>
  zoneTypes: Map<string, ZoneTypeDef>
  commonDefaults: Props
}

export function indexCatalog(catalog: Catalog): CatalogIndex {
  const commonDefaults: Props = {}
  for (const p of catalog.commonProps) if (p.default !== undefined && p.default !== null) commonDefaults[p.key] = p.default
  return {
    catalog,
    kinds: new Map(catalog.kinds.map((k) => [k.id, k])),
    edgeTypes: new Map(catalog.edgeTypes.map((e) => [e.id, e])),
    zoneTypes: new Map(catalog.zoneTypes.map((z) => [z.id, z])),
    commonDefaults,
  }
}

/** Catalog defaults overlaid with the values the user declared (null = not declared). */
export function effectiveProps(ix: CatalogIndex, kindId: string, declared: Props): Props {
  const out: Props = { ...ix.commonDefaults }
  const kind = ix.kinds.get(kindId)
  if (kind) for (const p of kind.props) if (p.default !== undefined && p.default !== null) out[p.key] = p.default
  for (const [k, v] of Object.entries(declared)) if (v !== null && v !== undefined) out[k] = v
  return out
}

export function edgeEffectiveProps(ix: CatalogIndex, typeId: string, declared: Props): Props {
  const out: Props = { ...(ix.edgeTypes.get(typeId)?.defaultProps ?? {}) }
  for (const p of ix.edgeTypes.get(typeId)?.props ?? []) if (p.default !== undefined && p.default !== null && !(p.key in out)) out[p.key] = p.default
  for (const [k, v] of Object.entries(declared)) if (v !== null && v !== undefined) out[k] = v
  return out
}

/** Property definitions applicable to a kind: its own first, then the common ones it does not redefine. */
export function kindPropDefs(ix: CatalogIndex, kindId: string): PropDef[] {
  const kind = ix.kinds.get(kindId)
  const own = kind?.props ?? []
  const seen = new Set(own.map((p) => p.key))
  return [...own, ...ix.catalog.commonProps.filter((p) => !seen.has(p.key))]
}

function interpolate(seg: string, vars: Record<string, Scalar | undefined>): string {
  return seg.replace(/\{(\w+)\}/g, (m, k) => {
    const v = vars[k]
    if (v === undefined || v === null || v === '') return m
    if (typeof v === 'number') return Number.isInteger(v) ? String(v) : String(Math.round(v * 100) / 100)
    return String(v)
  })
}

/** Mirror of the engine's `render_subtitle`: segments with unset properties are dropped. */
export function renderSubtitle(tpl: LText | null | undefined, eff: Props): string {
  if (!tpl) return ''
  return lt(tpl)
    .split(' · ')
    .map((seg) => interpolate(seg, eff))
    .filter((seg) => !seg.includes('{'))
    .join(' · ')
}

export function nodeSubtitle(ix: CatalogIndex, n: ArchNode): string {
  const kind = ix.kinds.get(n.kind)
  if (!kind) return ''
  return renderSubtitle(kind.subtitle, effectiveProps(ix, n.kind, n.props))
}

export function propLabel(p: PropDef): string {
  return lt(p.label)
}

export function optionLabel(p: PropDef, value: unknown): string {
  const o = p.options?.find((x) => x.value === String(value))
  return o ? lt(o.label) : String(value)
}

/** Value as shown in the compact card of the inspector. */
export function formatPropValue(p: PropDef, value: Scalar | undefined): string {
  if (value === undefined || value === null || value === '') return ''
  if (p.type === 'boolean') return value ? '✓' : '—'
  if (p.type === 'enum') return optionLabel(p, value)
  return p.unit ? `${value} ${p.unit}` : String(value)
}
