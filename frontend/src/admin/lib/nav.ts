// Structure of the admin sub-navigation.

import type { Component } from 'vue'
import {
  Activity, Bot, Boxes, DatabaseBackup, FileCode2, Gauge, LayoutTemplate, MessageSquareText, ScrollText, ShieldCheck, SlidersHorizontal, Users,
} from 'lucide-vue-next'

export interface NavItem {
  id: string
  to: string
  /** i18n key */
  label: string
  icon?: Component
  /** config kind whose counters are shown */
  kind?: string
  children?: NavItem[]
}

export interface NavGroup { id: string; label: string; items: NavItem[] }

export const NAV: NavGroup[] = [
  {
    id: 'service',
    label: 'admin.navGroup.service',
    items: [
      { id: 'settings', to: '/admin/settings', label: 'admin.nav.settings', icon: SlidersHorizontal },
      { id: 'llm', to: '/admin/llm', label: 'admin.nav.llm', icon: Bot },
    ],
  },
  {
    id: 'content',
    label: 'admin.navGroup.content',
    items: [
      {
        id: 'catalog', to: '/admin/catalog/kind', label: 'admin.nav.catalog', icon: Boxes,
        children: [
          { id: 'catalog/kind', to: '/admin/catalog/kind', label: 'admin.nav.catalogKind', kind: 'catalog.kind' },
          { id: 'catalog/edge_type', to: '/admin/catalog/edge_type', label: 'admin.nav.catalogEdge', kind: 'catalog.edge_type' },
          { id: 'catalog/zone_type', to: '/admin/catalog/zone_type', label: 'admin.nav.catalogZone', kind: 'catalog.zone_type' },
          { id: 'catalog/category', to: '/admin/catalog/category', label: 'admin.nav.catalogCategory', kind: 'catalog.category' },
          { id: 'catalog/versions', to: '/admin/catalog/versions', label: 'admin.nav.catalogVersions', kind: 'catalog.versions' },
          { id: 'catalog/common_props', to: '/admin/catalog/common_props', label: 'admin.nav.catalogCommon', kind: 'catalog.common_props' },
        ],
      },
      { id: 'rules', to: '/admin/rules', label: 'admin.nav.rules', icon: ShieldCheck, kind: 'rule' },
      { id: 'templates', to: '/admin/templates', label: 'admin.nav.templates', icon: LayoutTemplate, kind: 'template' },
      { id: 'iac', to: '/admin/iac', label: 'admin.nav.iac', icon: FileCode2, kind: 'iac' },
      { id: 'prompts', to: '/admin/prompts', label: 'admin.nav.prompts', icon: MessageSquareText, kind: 'prompt' },
      { id: 'engine', to: '/admin/engine', label: 'admin.nav.engine', icon: Gauge, kind: 'engine' },
    ],
  },
  {
    id: 'ops',
    label: 'admin.navGroup.ops',
    items: [
      { id: 'backup', to: '/admin/backup', label: 'admin.nav.backup', icon: DatabaseBackup },
      { id: 'users', to: '/admin/users', label: 'admin.nav.users', icon: Users },
      { id: 'audit', to: '/admin/audit', label: 'admin.nav.audit', icon: ScrollText },
      { id: 'system', to: '/admin/system', label: 'admin.nav.system', icon: Activity },
    ],
  },
]

/** Id of the nav item that is active for the given route params. */
export function activeNavId(section: string, sub?: string): string {
  if (section === 'catalog') return `catalog/${sub || 'kind'}`
  return section
}

/** Pending edits are tied to a nav item, except inside Settings where the drafts survive a group switch. */
export function guardKey(section: string, sub?: string): string {
  if (section === 'settings' || section === 'llm') return section
  return activeNavId(section, sub)
}
