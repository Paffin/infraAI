// A failure as a link: the scenario goes into the address (`?sim=`), whoever opens it gets the editor with that failure on screen.
// The scenario is plain data (ids of zones, nodes and links), so the link carries it as it is; what is checked on the way back is that it is
// a scenario at all and that what it names exists in the design it is opened on.

import type { Architecture, Scenario } from '@/api/types'

/** The name of the address parameter. */
export const SIM_PARAM = 'sim'

const TYPES = ['none', 'instance', 'node', 'unit', 'zone', 'link', 'isolate', 'sites', 'geo', 'combo'] as const
/** A link is an address: past this size a browser, a chat or a proxy cuts it. */
export const MAX_LINK_CHARS = 1800

const isId = (v: unknown): v is string => typeof v === 'string' && v.length > 0 && v.length <= 200
const isIds = (v: unknown): v is string[] => Array.isArray(v) && v.length > 0 && v.every(isId)

/** Is `v` a scenario of the shape the engine takes? (`depth` bounds the nesting of combinations.) */
export function isScenario(v: unknown, depth = 0): v is Scenario {
  if (!v || typeof v !== 'object' || depth > 4) return false
  const s = v as Record<string, unknown>
  if (!TYPES.includes(s.type as (typeof TYPES)[number])) return false
  switch (s.type) {
    case 'none': return true
    case 'instance': return isId(s.nodeId) && (s.count === undefined || (typeof s.count === 'number' && Number.isInteger(s.count) && s.count >= 1 && s.count <= 100000))
    case 'node': return isId(s.nodeId)
    case 'unit': return isId(s.unit)
    case 'zone': return isId(s.zoneId)
    case 'link': return isId(s.a) && isId(s.b)
    case 'isolate':
    case 'sites': return isIds(s.zoneIds)
    case 'geo': return isId(s.geo)
    default: return Array.isArray(s.items) && s.items.length > 0 && s.items.length <= 100 && s.items.every((x) => isScenario(x, depth + 1))
  }
}

const b64 = (bytes: Uint8Array) => btoa(String.fromCharCode(...bytes)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
const unb64 = (text: string) => Uint8Array.from(atob(text.replace(/-/g, '+').replace(/_/g, '/')), (c) => c.charCodeAt(0))

/** The scenario as the value of the address parameter (URL-safe, any language in the ids). */
export function encodeScenario(sc: Scenario): string {
  return b64(new TextEncoder().encode(JSON.stringify(sc)))
}

/** The scenario in a parameter value, or `null` when it is not one. */
export function decodeScenario(text: string | null | undefined): Scenario | null {
  if (!text || text.length > MAX_LINK_CHARS * 2) return null
  try {
    const v: unknown = JSON.parse(new TextDecoder().decode(unb64(text)))
    return isScenario(v) ? v : null
  } catch {
    return null
  }
}

/** Every zone, node and unit the scenario names exists in `arch`; `unitKeys` are the clusters the engine knows (they come from the groups of the design). */
export function scenarioFits(sc: Scenario, arch: Architecture, unitKeys?: ReadonlySet<string>): boolean {
  const zones = new Set(arch.zones.map((z) => z.id))
  const nodes = new Set(arch.nodes.map((n) => n.id))
  switch (sc.type) {
    case 'none': return true
    case 'instance':
    case 'node': return nodes.has(sc.nodeId)
    case 'unit': return !unitKeys || unitKeys.has(sc.unit)
    case 'zone': return zones.has(sc.zoneId)
    case 'link': return zones.has(sc.a) && zones.has(sc.b)
    case 'isolate':
    case 'sites': return sc.zoneIds.every((z) => zones.has(z))
    case 'geo': return arch.zones.some((z) => z.props.geo === sc.geo)
    case 'combo': return sc.items.every((x) => scenarioFits(x, arch, unitKeys))
  }
}

/** The address of the page that shows `sc`: this page's address with the scenario in it. */
export function linkTo(sc: Scenario, here: string): string | null {
  const url = new URL(here)
  url.search = ''
  url.hash = ''
  url.searchParams.set(SIM_PARAM, encodeScenario(sc))
  const out = url.toString()
  return out.length <= MAX_LINK_CHARS ? out : null
}
