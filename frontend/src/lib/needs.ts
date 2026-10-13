import type { Architecture, Need, PropDef } from '@/api/types'
import { has, lt, t } from '@/i18n'
import { kindPropDefs, type CatalogIndex } from './catalog'

export interface NeedView {
  /** what to fill in ("RTO", "Replicas per site"…) */
  prop: string
  /** where it lives ("Requirements", the node's name, "App DC1 → Postgres"…) */
  where: string
}

const nodeName = (arch: Architecture, id: string) => arch.nodes.find((n) => n.id === id)?.name ?? id

/** Property definition a need points at, or `undefined` when the catalog does not describe it. */
export function needDef(need: Need, arch: Architecture, ix: CatalogIndex): PropDef | undefined {
  if (need.target === 'node') {
    const n = arch.nodes.find((x) => x.id === need.id)
    return n ? kindPropDefs(ix, n.kind).find((d) => d.key === need.prop) : undefined
  }
  if (need.target === 'edge') {
    const e = arch.edges.find((x) => x.id === need.id)
    return e ? ix.edgeTypes.get(e.type)?.props.find((d) => d.key === need.prop) : undefined
  }
  if (need.target === 'zone') {
    const z = arch.zones.find((x) => x.id === need.id)
    return z ? ix.zoneTypes.get(z.type)?.props.find((d) => d.key === need.prop) : undefined
  }
  return undefined
}

/** Human description of a need: which field of which element. */
export function needView(need: Need, arch: Architecture, ix: CatalogIndex): NeedView {
  const id = need.id ?? ''
  switch (need.target) {
    case 'requirements':
      return { prop: has('req.' + need.prop) ? t('req.' + need.prop) : need.prop, where: t('tab.requirements') }
    case 'node':
      return { prop: label(needDef(need, arch, ix), need.prop), where: nodeName(arch, id) }
    case 'edge': {
      const e = arch.edges.find((x) => x.id === id)
      return { prop: label(needDef(need, arch, ix), need.prop), where: e ? `${nodeName(arch, e.source)} → ${nodeName(arch, e.target)}` : id }
    }
    case 'zone':
      return { prop: label(needDef(need, arch, ix), need.prop), where: arch.zones.find((z) => z.id === id)?.name ?? id }
  }
}

const label = (def: PropDef | undefined, key: string) => (def ? lt(def.label) : key)

/** Does the element a need points at still exist (the finding may be older than the last edit)? */
export function needExists(need: Need, arch: Architecture): boolean {
  if (need.target === 'requirements') return true
  const list = need.target === 'node' ? arch.nodes : need.target === 'edge' ? arch.edges : arch.zones
  return list.some((x) => x.id === need.id)
}

/** The same need twice (two findings, two subjects) is offered once. */
export function uniqueNeeds(needs: Need[]): Need[] {
  const seen = new Set<string>()
  return needs.filter((n) => {
    const k = `${n.target}:${n.id ?? ''}:${n.prop}`
    if (seen.has(k)) return false
    seen.add(k)
    return true
  })
}
