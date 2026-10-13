// JSON shapes of the backend API (see docs/API.md). Keep in sync with the Rust `engine` types.

export type LText = string | Record<string, string>
export type Scalar = string | number | boolean | null
export type Props = Record<string, Scalar>

// ---------------------------------------------------------------------------- architecture

export interface Ui { x?: number; y?: number; w?: number; h?: number }

export interface Requirements {
  rpsPeak?: number
  growthPercent?: number
  rtoSeconds?: number
  rpoSeconds?: number
  availabilitySlo?: number
  budgetMonthly?: number
  failureTolerance?: 'instance' | 'site' | 'geo'
  compliance?: string[]
  notes?: string
}

export interface Zone {
  id: string
  name: string
  type: string
  parentId: string | null
  role: string | null
  props: Props
  ui?: Ui
}

export interface ArchNode {
  id: string
  kind: string
  name: string
  zoneId: string | null
  group: string | null
  role: string | null
  props: Props
  ui?: Ui
  proposed?: boolean
}

export interface Edge {
  id: string
  source: string
  target: string
  type: string
  label: string | null
  props: Props
}

export interface Architecture {
  schemaVersion: number
  meta: { name?: string | null; description?: string | null }
  requirements: Requirements
  zones: Zone[]
  nodes: ArchNode[]
  edges: Edge[]
}

// ---------------------------------------------------------------------------- catalog

export type Behavior = 'stateless' | 'primary_replica' | 'consensus' | 'quorum_log'
export type PropType = 'number' | 'string' | 'boolean' | 'enum' | 'unit_ref'
export type Semantics = 'dependency' | 'replication' | 'membership' | 'backup' | 'control'
export type ZoneLevel = 'group' | 'rack' | 'site' | 'geo'

export interface PropOption { value: string; label: LText }

export interface PropDef {
  key: string
  type: PropType
  label: LText
  unit?: string | null
  min?: number | null
  max?: number | null
  options?: PropOption[]
  default?: Scalar
  group?: string | null
  help?: LText | null
  refBehavior?: Behavior | null
  advanced?: boolean
}

export interface KindDef {
  id: string
  category: string
  name: LText
  description?: LText | null
  abbr: string
  color: string
  behavior: Behavior
  traits: string[]
  roles: { id: string; name: LText }[]
  props: PropDef[]
  subtitle?: LText | null
  capacity?: { rpsPerReplica?: number; rpsPerVcpu?: number } | null
  hidden?: boolean
}

export interface EdgeTypeDef {
  id: string
  name: LText
  short?: LText | null
  semantics: Semantics
  style: { color: string; dash?: string | null; width?: number | null }
  directed: boolean
  defaultProps: Props
  props: PropDef[]
  layer?: string | null
}

export interface ZoneTypeDef { id: string; name: LText; level: ZoneLevel; color: string; props: PropDef[] }
export interface Category { id: string; name: LText; order: number }

export interface Catalog {
  categories: Category[]
  zoneTypes: ZoneTypeDef[]
  edgeTypes: EdgeTypeDef[]
  commonProps: PropDef[]
  kinds: KindDef[]
}

export interface Rule {
  id: string
  category: string
  check: string
  enabled: boolean
  severity: 'risk' | 'critical'
  weight?: number | null
  params: Record<string, unknown>
  title: LText
  message: LText
  noDataMessage?: LText | null
  requirementMessage?: LText | null
  dataLostMessage?: LText | null
  ok: LText
  rationale?: LText | null
  remediation?: LText | null
  fixLabels: Record<string, LText>
  references: string[]
}

// ---------------------------------------------------------------------------- operations

export type Op =
  | { op: 'add_zone'; zone: Zone }
  | { op: 'update_zone'; id: string; set: Record<string, unknown> }
  | { op: 'remove_zone'; id: string; cascade?: boolean }
  | { op: 'add_node'; node: ArchNode }
  | { op: 'update_node'; id: string; set: Record<string, unknown> }
  | { op: 'remove_node'; id: string }
  | { op: 'add_edge'; edge: Edge }
  | { op: 'update_edge'; id: string; set: Record<string, unknown> }
  | { op: 'remove_edge'; id: string }
  | { op: 'set_requirements'; set: Record<string, unknown> }

export interface Change { path: string; from: unknown; to: unknown }
export interface EntityRef { id: string; name: string }
export interface EntityChange { id: string; name: string; changes: Change[] }
export interface EntityDiff { added: EntityRef[]; removed: EntityRef[]; changed: EntityChange[] }
export interface ArchDiff { requirements: Change[]; zones: EntityDiff; nodes: EntityDiff; edges: EntityDiff }

// ---------------------------------------------------------------------------- report

export type Status = 'critical' | 'risk' | 'no_data' | 'ok' | 'na'

export type Scenario =
  | { type: 'none' }
  | { type: 'instance'; nodeId: string; count?: number }
  | { type: 'node'; nodeId: string }
  | { type: 'unit'; unit: string }
  | { type: 'zone'; zoneId: string }
  | { type: 'link'; a: string; b: string }
  | { type: 'sites'; zoneIds: string[] }
  | { type: 'geo'; geo: string }

export interface Fix { id: string; kind: string; vars: Record<string, unknown>; ops: Op[] }

/** A value the user can supply to turn a "no data" finding into a verified one: the UI opens exactly that field. */
export interface Need {
  target: 'requirements' | 'node' | 'edge' | 'zone'
  /** entity id (absent for requirements) */
  id?: string
  /** property key; for requirements the field name (`rtoSeconds`, `rpsPeak`…) */
  prop: string
}

export interface Finding {
  id: string
  ruleId: string
  category: string
  status: Status
  subjects: { nodes: string[]; edges: string[]; zones: string[] }
  vars: Record<string, unknown>
  fixes: Fix[]
  /** what to fill in to verify a `no_data` finding */
  needs?: Need[]
  scenario?: Scenario
  accepted?: { reason: string; by?: string | null; at?: string | null } | null
}

export interface RuleResult { ruleId: string; category: string; status: Status; findings: number; accepted: number }
export interface Counts { critical: number; risk: number; noData: number; ok: number; na: number; accepted: number }

export interface Objective {
  id: 'rto' | 'rpo' | 'site_loss' | 'spof' | 'capacity' | string
  status: Status
  required?: unknown
  actual?: unknown
  assumed: boolean
}

export type ServiceState = 'up' | 'degraded' | 'down'
export type RecoveryKind = 'none' | 'auto' | 'manual' | 'never'

export interface ScenarioSummary {
  scenario: Scenario
  failed: string[]
  state: ServiceState
  recoveryKind: RecoveryKind
  recoverySeconds: number | null
  rpoSeconds: number | null
  rpoAssumed: boolean
  capacityRatio: number | null
  splitBrain: boolean
  dataLost: boolean
  downUnits: string[]
}

export interface UnitLoad { unit: string; loadRps: number; capacityRps: number | null; ratio: number | null }

export interface CapacityWorst { scenario: Scenario; failed: string[]; loadRps: number; capacityRps: number }

/** Capacity plan of one logical service (`Report.capacity`); load scales linearly with the peak RPS. */
export interface CapacityRow {
  unit: string
  name: string
  kind: string
  nodes: string[]
  /** instances that serve load (arbiters excluded) */
  instances: number
  perReplicaRps: number | null
  loadRps: number
  /** `null` = no capacity model (set vCPU or RPS per instance) */
  capacityRps: number | null
  /** capacity needed to keep the configured headroom at peak plus growth */
  targetRps: number
  instancesNeeded: number | null
  /** the failure scenario that leaves the service the least capacity relative to its load */
  worst: CapacityWorst | null
}

export interface CostReport {
  currency: string
  symbol: string
  total: number
  nodes: { nodeId: string; amount: number; missing: string[]; model: string }[]
  byCategory: Record<string, number>
  byZone: Record<string, number>
  byKind: Record<string, number>
  incomplete: number
  budget?: number | null
}

export interface Issue { level: 'error' | 'warning'; code: string; message: string; subject?: string }

export interface Report {
  engineVersion: number
  issues: Issue[]
  counts: Counts
  rules: RuleResult[]
  findings: Finding[]
  objectives: Objective[]
  scenarios: ScenarioSummary[]
  baselineLoads: UnitLoad[]
  capacity: CapacityRow[]
  score: { value: number; penalty: number; coverage: number; breakdown: { ruleId: string; status: Status; penalty: number }[] }
  cost: CostReport
  units: Record<string, string>
}

export interface SimEvent { code: string; unit?: string; node?: string; vars?: Record<string, unknown> }
export type NodeState = 'up' | 'partial' | 'down' | 'stalled' | 'fenced' | 'promoted' | 'isolated'
export type EdgeState = 'ok' | 'cut' | 'dead'

export interface SimResult {
  scenario: Scenario
  state: ServiceState
  recovery: { kind: RecoveryKind; seconds: number | null }
  rpoSeconds: number | null
  rpoAssumed: boolean
  rpoItems: { unit: string; seconds: number | null; assumed: boolean; traits: string[] }[]
  dataLost: string[]
  splitBrain: string[]
  capacityRatio: number | null
  bottleneck: string | null
  unitLoads: UnitLoad[]
  downUnits: string[]
  events: SimEvent[]
  failed: string[]
  nodeStates: Record<string, NodeState>
  edgeStates: Record<string, EdgeState>
}

export interface ReportSummary {
  score: number
  coverage: number
  counts: Counts
  objectives: [string, Status][]
  costTotal: number
  open: string[]
}

// ---------------------------------------------------------------------------- app

export type Role = 'viewer' | 'editor' | 'admin'
export interface User { id: string; email: string; name: string; role: Role; mustChangePassword: boolean }

export interface TemplateMeta {
  id: string
  name: LText
  description?: LText | null
  badge?: string | null
  order: number
  nodeCount: number
  requirements: Requirements
}

export interface Template extends Omit<TemplateMeta, 'nodeCount' | 'requirements'> { architecture: Architecture }

export interface Bootstrap {
  user: User
  rev: number
  version: string
  engineVersion: number
  app: { name: string; defaultLanguage: string; languages: string[]; theme: Record<string, string>; quickPrompts: { label: LText; prompt: LText }[] }
  catalog: Catalog
  rules: Rule[]
  templates: TemplateMeta[]
  exportFormats: ExportFormatInfo[]
  ai: { enabled: boolean; configured: boolean; maxPromptChars: number; plannerModel: string; fastModel: string }
  limits: { maxNodes: number; maxEdges: number; maxZones: number; waiverMinReasonChars: number; waiverMaxDays: number }
  engine: { currency: string; symbol: string; minCapacityDuringFailure: number; headroomPercent: number; defaultFailureTolerance: string }
}

/** What the project list ships to draw a thumbnail (coordinates are `null` until the editor has laid the diagram out). */
export interface ProjectThumb {
  zones: { i: string; x: number | null; y: number | null; w: number | null; h: number | null; t: string; p: string | null }[]
  nodes: { i: string; x: number | null; y: number | null; k: string; z: string | null }[]
  /** `[source, target, type]` */
  edges: [string, string, string][]
}

export interface ProjectListItem {
  id: string
  name: string
  description: string
  owner: string | null
  updatedAt: string
  createdAt: string
  headVersion: number
  draftRev: number
  nodeCount: number
  summary: ReportSummary | null
  thumb: ProjectThumb | null
}

export interface Waiver { id: string; fingerprint: string; ruleId: string; reason: string; createdBy: string; createdAt: string; expiresAt: string | null }

export interface ProjectDetail {
  project: { id: string; name: string; description: string; owner: string | null; headVersion: number; updatedAt: string; createdAt: string }
  architecture: Architecture
  rev: number
  waivers: Waiver[]
}

export interface VersionItem { n: number; message: string; author: string; summary: ReportSummary; createdAt: string }

export interface AiStep { code: string; detail: Record<string, unknown>; at: string }
export interface AiJob {
  id: string
  projectId: string | null
  kind: string
  status: 'queued' | 'running' | 'succeeded' | 'failed' | 'cancelled'
  steps: AiStep[]
  result: Record<string, unknown> | null
  error: string | null
  errorCode: string | null
  createdAt: string
  finishedAt: string | null
}

export interface ChatMessage {
  id: number
  role: 'user' | 'assistant' | 'system'
  content: string
  proposalId: string | null
  proposalStatus: 'pending' | 'applied' | 'rejected' | 'stale' | null
  createdAt: string
}

export interface Proposal {
  id: string
  projectId: string
  baseRev: number
  status: 'pending' | 'applied' | 'rejected' | 'stale'
  mode: 'ops' | 'replace'
  ops?: Op[] | null
  architecture?: Architecture | null
  rationale: string
  preview: { diff: ArchDiff; before: ReportSummary; after: ReportSummary; introduced: string[]; resolved: string[] }
  createdAt: string
}

export interface ExportFormatInfo { id: string; group: 'iac' | 'docs' | 'data' }

export interface IacCluster { nodes: string[]; name: string; slug: string; zones: string[] }
export interface IacGenerated { unit: string; name: string; kind: string; cluster: string; nodes: string[]; items: string[] }
export interface IacSkipped { unit: string; name: string; kind: string; nodes: string[]; code: string; message: string; props: string[] }
export interface IacNote { level: 'info' | 'warn'; unit: string | null; message: string }
export interface IacPrerequisite { cluster: string; id: string; title: string; release: string; namespace: string; chart: string; repo: string; version: string }
export interface IacSecret { cluster: string; namespace: string; name: string; keys: string[]; unit: string; hint: string | null }
/** What an infrastructure-as-code export did and did not produce. */
export interface IacReport {
  clusters: IacCluster[]
  generated: IacGenerated[]
  skipped: IacSkipped[]
  notes: IacNote[]
  prerequisites: IacPrerequisite[]
  secrets: IacSecret[]
}

export interface ExportFile { path: string; content: string }

// ---------------------------------------------------------------------------- admin

export interface SettingDef {
  key: string
  group: string
  type: 'string' | 'number' | 'boolean' | 'enum' | 'json' | 'secret' | 'string_list' | 'model'
  default: unknown
  label: LText
  help?: LText | null
  options?: string[] | null
  min?: number | null
  max?: number | null
  // present in the admin listing
  value?: unknown
  modified?: boolean
  secretSet?: boolean
  secretSource?: 'database' | 'environment' | 'none'
}

export interface ConfigItem {
  id: string
  data: any
  origin: 'default' | 'custom'
  modified: boolean
  updateAvailable: boolean
  hasDefault: boolean
  updatedAt: string
  updatedBy: string
}

export interface LlmModel {
  id: string
  name: string
  contextLength: number | null
  promptPerMillion: number | null
  completionPerMillion: number | null
  structuredOutputs: boolean
  responseFormat: boolean
  supportedParameters: string[]
}

export interface AdminUser {
  id: string
  email: string
  name: string
  role: Role
  disabled: boolean
  mustChangePassword: boolean
  createdAt: string
  lastLoginAt: string | null
  lockedUntil: string | null
}

export interface AuditEvent {
  id: number
  at: string
  actor: string
  action: string
  targetType: string
  targetId: string
  details: Record<string, unknown>
  ip: string
}

export interface ConfigKindCounter { kind: string; count: number; modified: number; custom: number; updatesAvailable: number }
export interface ConfigError { kind: string; id: string; error: string }

export interface LlmKeyInfo { label: string | null; limit: number | null; limitRemaining: number | null; usage: number | null; isFreeTier: boolean | null }
export interface LlmModelCheck { id: string; found: boolean; structuredOutputs: boolean | null; contextLength: number | null }
export interface LlmPing { ok: boolean; model?: string; latencyMs?: number; costUsd?: number | null; answer?: string; code?: string; message?: string }
/** `POST /admin/llm/test`; without a key only `ok`, `code` and `message` are present. */
export interface LlmTestResult {
  ok: boolean
  message?: string
  code?: string
  key?: LlmKeyInfo | null
  keySource?: 'database' | 'environment'
  warnings?: string[]
  /** per role (`planner`, `fast`) */
  models?: Record<string, LlmModelCheck>
  ping?: LlmPing | null
}

export interface LlmUsageDay { day: string; calls: number; errors: number; promptTokens: number; completionTokens: number; costUsd: number }
export interface LlmUsageByModel { model: string; calls: number; costUsd: number; avgLatencyMs: number }
export interface LlmUsageByUser { user: string; calls: number; costUsd: number }
export interface LlmUsage {
  monthSpendUsd: number
  monthlyBudgetUsd: number
  daily: LlmUsageDay[]
  byModel: LlmUsageByModel[]
  byUser: LlmUsageByUser[]
}

export interface ConfigExport {
  format: string
  exportedAt: string
  settings: Record<string, unknown>
  items: { kind: string; id: string; data: unknown; origin: 'default' | 'custom' }[]
}
export interface ConfigImportResult { ok: boolean; problems: string[]; items: number; settings: number; applied: boolean }

/** `GET /admin/system` */
export interface SystemInfo {
  version: string
  engineVersion: number
  instance: string
  uptimeSeconds: number
  production: boolean
  db: { ok: boolean; latencyMs: number; version: string; migrations: number; poolSize: number; poolIdle: number }
  configRev: number
  configErrors: ConfigError[]
  jobsLast24h: { status: string; count: number }[]
  counts: { projects: number; users: number }
  llm: { enabled: boolean; keyConfigured: boolean; keySource: 'database' | 'environment' | 'none' }
  env: Record<string, string | number | boolean>
}
