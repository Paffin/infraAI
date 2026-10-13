// The icons of the diagram: the catalog says which one a kind, a zone type or a category wears by name (`icon` in the catalog items), and
// this is the set of names the UI knows. A name that is not here (a typo in an item an administrator wrote) falls back to the icon of the
// category, then to a plain box: the card never has a hole where the icon should be. The list is in docs/CATALOG.md.

import type { Component } from 'vue'
import {
  Activity, ArchiveRestore, ArrowRightLeft, BadgeCheck, Blocks, Box, Boxes, BrickWall, Building2, Cable, ChartColumn, ChartLine, Cloud, CloudDownload, Columns3, Cpu, Database, DatabaseZap,
  FileBraces, FingerprintPattern, GitBranch, GitFork, GitPullRequest, Globe, HardDrive, Hash, IdCard, KeyRound, Layers, LayoutDashboard, LogIn, MessagesSquare, Network, Package, Plug, Radar,
  Route, Router, Scale, ScrollText, Search, Server, Share2, ShieldCheck, Signpost, Split, SquareFunction, Users, Vote, Waypoints, Workflow, Zap,
  // a few more for the administrator's own kinds
  Bell, Bot, Brain, Clock, Cog, Container, FileText, Fingerprint, Folder, Gauge, HardDriveDownload, Inbox, Layers2, Link, Lock, Mail, MonitorCog, Radio, Repeat, Rss, ServerCog, Settings2, Shield, Smartphone, Terminal, Wifi,
} from 'lucide-vue-next'

export const ICONS: Record<string, Component> = {
  activity: Activity, 'archive-restore': ArchiveRestore, 'arrow-right-left': ArrowRightLeft, 'badge-check': BadgeCheck, blocks: Blocks, box: Box, boxes: Boxes, 'brick-wall': BrickWall, 'building-2': Building2,
  cable: Cable, 'chart-column': ChartColumn, 'chart-line': ChartLine, cloud: Cloud, 'cloud-download': CloudDownload, 'columns-3': Columns3, cpu: Cpu, database: Database, 'database-zap': DatabaseZap,
  'file-braces': FileBraces, 'fingerprint-pattern': FingerprintPattern, 'git-branch': GitBranch, 'git-fork': GitFork, 'git-pull-request': GitPullRequest, globe: Globe, 'hard-drive': HardDrive, hash: Hash,
  'id-card': IdCard, 'key-round': KeyRound, layers: Layers, 'layout-dashboard': LayoutDashboard, 'log-in': LogIn, 'messages-square': MessagesSquare, network: Network, package: Package, plug: Plug, radar: Radar,
  route: Route, router: Router, scale: Scale, 'scroll-text': ScrollText, search: Search, server: Server, 'share-2': Share2, 'shield-check': ShieldCheck, signpost: Signpost, split: Split, 'square-function': SquareFunction,
  users: Users, vote: Vote, waypoints: Waypoints, workflow: Workflow, zap: Zap,
  bell: Bell, bot: Bot, brain: Brain, clock: Clock, cog: Cog, container: Container, 'file-text': FileText, fingerprint: Fingerprint, folder: Folder, gauge: Gauge, 'hard-drive-download': HardDriveDownload,
  inbox: Inbox, 'layers-2': Layers2, link: Link, lock: Lock, mail: Mail, 'monitor-cog': MonitorCog, radio: Radio, repeat: Repeat, rss: Rss, 'server-cog': ServerCog, 'settings-2': Settings2, shield: Shield,
  smartphone: Smartphone, terminal: Terminal, wifi: Wifi,
}

/** The icon for a name, or for the first fallback that is known, or a box. */
export function iconFor(...names: (string | null | undefined)[]): Component {
  for (const n of names) if (n && ICONS[n]) return ICONS[n]
  return Box
}
