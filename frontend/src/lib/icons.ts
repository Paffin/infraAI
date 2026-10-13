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

import {
  // the network, the edge and the sites
  Antenna, RadioTower, SatelliteDish, Signal, EthernetPort, PlugZap, GlobeLock, ShieldAlert, ShieldBan, LockKeyhole, Funnel, Merge, Timer, MapPin,
  // compute and platforms
  Microchip, MemoryStick, ServerCrash, PackageOpen, Component as ComponentIcon, Puzzle, Rocket, CloudCog, CloudUpload, CloudLightning,
  // data, storage and analytics
  DatabaseBackup, Table, Table2, Sheet, FileSpreadsheet, ChartArea, ChartBar, ChartPie, ChartNoAxesCombined, Binary, Brackets, HardDriveUpload, Archive, FolderTree, FolderSync, Files, Save, Import,
  // the platform: delivery, tools, tests, configuration
  GitMerge, GitCompare, GitCommitHorizontal, SquareTerminal, Code, FileCode, Bug, BugPlay, FlaskConical, TestTube, Wrench, Hammer, SlidersHorizontal, ToggleRight, Flag, Stamp, BookOpen, Library, Notebook, ClipboardCheck, ListChecks, Infinity, Recycle, RefreshCw, History, CalendarClock,
  // security
  Siren, ScanSearch, SearchCheck, Eye, KeySquare, UserLock, UserCheck, UsersRound, Ban, Skull, Biohazard, BugOff, DoorClosed, ScanFace, Vault, FileLock, ShieldHalf, ShieldOff, Key,
  // applications and services
  Mailbox, Send, Phone, PhoneCall, Video, MessageCircle, Ticket, Briefcase, Building, Store, ShoppingCart, CreditCard, Wallet, Landmark, Factory, Warehouse, Truck, GraduationCap, Newspaper, BookText, AppWindow, LayoutGrid, Monitor, MonitorPlay, Tv, Laptop, Tablet, Printer, Headphones, Podcast, Clapperboard, Camera, Calendar, Contact, Handshake, Receipt, Calculator, ShoppingBag, HeartPulse,
  // AI and machine learning
  BrainCircuit, Sparkles, WandSparkles, Microscope, Atom, Tag, Tags, Image, Scan, BotMessageSquare, MessageSquareCode, NotebookPen,
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
  // the network, the edge and the sites
  antenna: Antenna, 'radio-tower': RadioTower, 'satellite-dish': SatelliteDish, signal: Signal, 'ethernet-port': EthernetPort, 'plug-zap': PlugZap,
  'globe-lock': GlobeLock, 'shield-alert': ShieldAlert, 'shield-ban': ShieldBan, 'lock-keyhole': LockKeyhole, funnel: Funnel, merge: Merge, timer: Timer,
  'map-pin': MapPin,
  // compute and platforms
  microchip: Microchip, 'memory-stick': MemoryStick, 'server-crash': ServerCrash, 'package-open': PackageOpen, component: ComponentIcon, puzzle: Puzzle, rocket: Rocket,
  'cloud-cog': CloudCog, 'cloud-upload': CloudUpload, 'cloud-lightning': CloudLightning,
  // data, storage and analytics
  'database-backup': DatabaseBackup, table: Table, 'table-2': Table2, sheet: Sheet, 'file-spreadsheet': FileSpreadsheet, 'chart-area': ChartArea, 'chart-bar': ChartBar,
  'chart-pie': ChartPie, 'chart-no-axes-combined': ChartNoAxesCombined, binary: Binary, brackets: Brackets, 'hard-drive-upload': HardDriveUpload, archive: Archive,
  'folder-tree': FolderTree, 'folder-sync': FolderSync, files: Files, save: Save, import: Import,
  // the platform: delivery, tools, tests, configuration
  'git-merge': GitMerge, 'git-compare': GitCompare, 'git-commit-horizontal': GitCommitHorizontal, 'square-terminal': SquareTerminal, code: Code, 'file-code': FileCode,
  bug: Bug, 'bug-play': BugPlay, 'flask-conical': FlaskConical, 'test-tube': TestTube, wrench: Wrench, hammer: Hammer, 'sliders-horizontal': SlidersHorizontal,
  'toggle-right': ToggleRight, flag: Flag, stamp: Stamp, 'book-open': BookOpen, library: Library, notebook: Notebook, 'clipboard-check': ClipboardCheck,
  'list-checks': ListChecks, infinity: Infinity, recycle: Recycle, 'refresh-cw': RefreshCw, history: History, 'calendar-clock': CalendarClock,
  // security
  siren: Siren, 'scan-search': ScanSearch, 'search-check': SearchCheck, eye: Eye, 'key-square': KeySquare, 'user-lock': UserLock, 'user-check': UserCheck,
  'users-round': UsersRound, ban: Ban, skull: Skull, biohazard: Biohazard, 'bug-off': BugOff, 'door-closed': DoorClosed, 'scan-face': ScanFace, vault: Vault,
  'file-lock': FileLock, 'shield-half': ShieldHalf, 'shield-off': ShieldOff, key: Key,
  // applications and services
  mailbox: Mailbox, send: Send, phone: Phone, 'phone-call': PhoneCall, video: Video, 'message-circle': MessageCircle, ticket: Ticket, briefcase: Briefcase,
  building: Building, store: Store, 'shopping-cart': ShoppingCart, 'credit-card': CreditCard, wallet: Wallet, landmark: Landmark, factory: Factory, warehouse: Warehouse,
  truck: Truck, 'graduation-cap': GraduationCap, newspaper: Newspaper, 'book-text': BookText, 'app-window': AppWindow, 'layout-grid': LayoutGrid, monitor: Monitor,
  'monitor-play': MonitorPlay, tv: Tv, laptop: Laptop, tablet: Tablet, printer: Printer, headphones: Headphones, podcast: Podcast, clapperboard: Clapperboard,
  camera: Camera, calendar: Calendar, contact: Contact, handshake: Handshake, receipt: Receipt, calculator: Calculator, 'shopping-bag': ShoppingBag,
  'heart-pulse': HeartPulse,
  // AI and machine learning
  'brain-circuit': BrainCircuit, sparkles: Sparkles, 'wand-sparkles': WandSparkles, microscope: Microscope, atom: Atom, tag: Tag, tags: Tags, image: Image, scan: Scan,
  'bot-message-square': BotMessageSquare, 'message-square-code': MessageSquareCode, 'notebook-pen': NotebookPen,
}

/** The icon for a name, or for the first fallback that is known, or a box. */
export function iconFor(...names: (string | null | undefined)[]): Component {
  for (const n of names) if (n && ICONS[n]) return ICONS[n]
  return Box
}
