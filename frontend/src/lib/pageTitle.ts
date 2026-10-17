import { normalizeSection, SECTION_SUBS } from '@/admin/lib/sections'
import { t } from '@/i18n'

/**
 * The title of the browser tab / window: what page this is, then the name of the service. With several tabs open, "Infra Architect" in each of them
 * says nothing; the project's name or the admin section does. One function for all of it: the router, the project's name, the language and the service's
 * name (a setting) all change what the title is, and a title written by whoever happened to load last would show the wrong one.
 */
export interface TitleInput {
  routeName: string | symbol | null | undefined
  params: Record<string, unknown>
  /** The name of the project that is open, when it is the one the route points at (not the previous one, still on screen while the next loads). */
  projectName?: string | null
  appName: string
}

/** The pages of the admin panel that have sub-pages, and the dictionary key of each. */
const SUB_KEYS: Record<string, Record<string, string>> = {
  catalog: { kind: 'admin.nav.catalogKind', edge_type: 'admin.nav.catalogEdge', zone_type: 'admin.nav.catalogZone', category: 'admin.nav.catalogCategory', versions: 'admin.nav.catalogVersions', common_props: 'admin.nav.catalogCommon' },
  placement: { pattern: 'admin.nav.placementPattern', stack: 'admin.nav.placementStack', settings: 'admin.nav.placementSettings' },
}
const text = (v: unknown) => (Array.isArray(v) ? String(v[0] ?? '') : v === undefined || v === null ? '' : String(v))

/** "{page} · {app}": the separator is one translatable string. Several parts fold from the right: "A · B · C" is A · (B · C). */
function join(parts: string[], appName: string): string {
  let title = appName
  for (let i = parts.length - 1; i >= 0; i--) if (parts[i]) title = t('page.title', { page: parts[i], app: title })
  return title
}

export function pageTitle(i: TitleInput): string {
  switch (i.routeName) {
    case 'login': return join([t('login.title')], i.appName)
    case 'projects': return join([t('nav.projects')], i.appName)
    case 'editor': return i.projectName ? join([i.projectName], i.appName) : i.appName
    case 'admin': {
      // an address that names no section (or one that does not exist) shows Settings, as the page itself does
      const section = normalizeSection(i.params.section)
      // `/admin/catalog` opens the first of its pages
      const sub = text(i.params.sub) || SECTION_SUBS[section] || ''
      const subKey = SUB_KEYS[section]?.[sub]
      return join([t('nav.admin'), t('admin.nav.' + section), subKey ? t(subKey) : ''], i.appName)
    }
    default: return i.appName
  }
}
