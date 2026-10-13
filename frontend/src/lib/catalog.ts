import type { ArchNode, Behavior, Catalog, EdgeTypeDef, FunctionDef, KindDef, LText, PropDef, Props, Scalar, VariantDef, ZoneTypeDef } from '@/api/types'
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

/** The variant a node of `kind` runs: the one it names, else the kind's default one (undefined for a kind without variants). */
export function variantOf(kind: KindDef | undefined, id?: Scalar | null): VariantDef | undefined {
  const vs = kind?.variants
  if (!vs?.length) return undefined
  const named = typeof id === 'string' && id ? vs.find((v) => v.id === id) : undefined
  return named ?? vs.find((v) => v.default)
}

/** Behavior of a node: the kind's, unless its variant changes it. */
export function behaviorOf(kind: KindDef | undefined, variant: VariantDef | undefined): Behavior | undefined {
  return variant?.behavior ?? kind?.behavior
}

/** Traits of a node: the kind's, plus what its variant adds, minus what it removes. */
export function traitsOf(kind: KindDef | undefined, variant: VariantDef | undefined): Set<string> {
  const out = new Set(kind?.traits ?? [])
  for (const t of variant?.addTraits ?? []) out.add(t)
  for (const t of variant?.removeTraits ?? []) out.delete(t)
  return out
}

/** Catalog defaults overlaid with the variant's and then with the values the user declared (null = not declared). */
export function effectiveProps(ix: CatalogIndex, kindId: string, declared: Props): Props {
  const out: Props = { ...ix.commonDefaults }
  const kind = ix.kinds.get(kindId)
  if (kind) for (const p of kind.props) if (p.default !== undefined && p.default !== null) out[p.key] = p.default
  const variant = variantOf(kind, declared.variant)
  if (variant?.defaults) for (const [k, v] of Object.entries(variant.defaults)) if (v !== null && v !== undefined) out[k] = v as Scalar
  for (const [k, v] of Object.entries(declared)) if (v !== null && v !== undefined) out[k] = v
  return out
}

/** The id of the release table that judges the version of a node (null = none): the variant's, else the kind's. */
export function tableIdOf(kind: KindDef | undefined, variant: VariantDef | undefined): string | null {
  if (!kind) return null
  const id = variant?.versions ?? kind.versions ?? kind.id
  return id || null
}

/** The functions of a category in the order the library shows them, each with its products (hidden ones left out). */
export interface FunctionGroup { id: string; name: FunctionDef['name']; kinds: KindDef[] }
export function functionGroups(catalog: Catalog, categoryId: string, filter: (k: KindDef) => boolean = () => true): FunctionGroup[] {
  const cat = catalog.categories.find((c) => c.id === categoryId)
  const kinds = catalog.kinds.filter((k) => !k.hidden && k.category === categoryId && filter(k))
  const out: FunctionGroup[] = []
  const used = new Set<string>()
  for (const f of cat?.functions ?? []) {
    const ks = kinds.filter((k) => k.function === f.id)
    if (ks.length) out.push({ id: f.id, name: f.name, kinds: ks })
    for (const k of ks) used.add(k.id)
  }
  // products without a (declared) function are not lost: they go last, under no heading
  const rest = kinds.filter((k) => !used.has(k.id))
  if (rest.length) out.push({ id: '', name: { ru: '', en: '' }, kinds: rest })
  return out
}

export function edgeEffectiveProps(ix: CatalogIndex, typeId: string, declared: Props): Props {
  const out: Props = { ...(ix.edgeTypes.get(typeId)?.defaultProps ?? {}) }
  for (const p of ix.edgeTypes.get(typeId)?.props ?? []) if (p.default !== undefined && p.default !== null && !(p.key in out)) out[p.key] = p.default
  for (const [k, v] of Object.entries(declared)) if (v !== null && v !== undefined) out[k] = v
  return out
}

/** Property definitions applicable to a kind: its own first, then the common ones it does not redefine. Properties that
 * name variants are shown only for those (a DCS is a Patroni matter, not a managed service's). */
export function kindPropDefs(ix: CatalogIndex, kindId: string, variantId?: Scalar | null): PropDef[] {
  const kind = ix.kinds.get(kindId)
  const variant = variantOf(kind, variantId)
  const own = (kind?.props ?? []).filter((p) => !p.variants?.length || (variant && p.variants.includes(variant.id)))
  const seen = new Set((kind?.props ?? []).map((p) => p.key))
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

/** Mirror of the engine's `render_subtitle`: segments with unset properties are dropped (`{variantName}` is the name of the variant). */
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
  const variant = variantOf(kind, n.props.variant)
  const eff = effectiveProps(ix, n.kind, n.props)
  return renderSubtitle(kind.subtitle, variant ? { ...eff, variantName: lt(variant.name) } : eff)
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
