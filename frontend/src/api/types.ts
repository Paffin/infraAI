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
  /** a level of the engine's tolerance ladder (`instance`, `rack`, `site`, `geo`, …) */
  failureTolerance?: string
  /** sites that may fail at the same time */
  siteFailures?: number
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
  /** For unit_ref: only units whose kind has this trait are offered (`dcs-capable`). */
  refTrait?: string | null
  advanced?: boolean
  /** Only for these variants of the kind (empty = every variant). */
  variants?: string[]
  /** The prop holds a version of a product with a release table: the table id, or `@key` to take it from another prop. */
  lifecycle?: string | null
}

/** A way to deploy a product: an HA topology or a managed offering. Everything it does not say is inherited from the kind. */
export interface VariantDef {
  id: string
  name: LText
  description?: LText | null
  default?: boolean
  behavior?: Behavior | null
  addTraits?: string[]
  removeTraits?: string[]
  defaults?: Props
  capacity?: { rpsPerReplica?: number; rpsPerVcpu?: number } | null
  cost?: { monthlyFixed?: number; multiplier?: number } | null
  /** Release table of this variant when it differs from the kind's; '' = none. */
  versions?: string | null
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
  /** What the product is for (`rdbms`, `cache`…): products of one function replace each other. */
  function?: string | null
  variants?: VariantDef[]
  /** Release table id when it differs from the kind id; '' = none. */
  versions?: string | null
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
export interface FunctionDef { id: string; name: LText; description?: LText | null }
export interface Category { id: string; name: LText; order: number; functions?: FunctionDef[] }

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
  /** Texts for one kind of problem of the rule, keyed by the finding's `variant` var. */
  variantMessages?: Record<string, LText>
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
  | { type: 'isolate'; zoneIds: string[] }
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
  id: 'rto' | 'rpo' | 'site_loss' | 'sites_loss' | 'spof' | 'capacity' | 'availability' | string
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

/** How much of the scenario space an evaluation looked at (the engine caps the combinations of many sites and says so). */
export interface ScenarioStats {
  generated: number
  listed: number
  truncated: boolean
  sites: number
  racks: number
  siteFailures: number
}

/** What fails: one instance, a whole service (a bad release), a service a provider runs, a rack, a site, the links, an isolated site, a disaster. */
export type AvailabilityCause = 'instance' | 'unit' | 'provider' | 'rack' | 'site' | 'link' | 'isolation' | 'geo'

/** One kind of failure and what it costs a year. `basis`: `repair` (nothing recovers it), `recovery` (a restore, an operator), `interruption` (a failover), `capacity` (what is left cannot carry the load). */
export interface AvailabilityFailure {
  scenario: Scenario
  cause: AvailabilityCause
  failed: string[]
  perYear: number
  downtimeSeconds: number
  secondsPerYear: number
  basis: 'repair' | 'recovery' | 'interruption' | 'capacity' | string
}

/** The assumptions the estimate rests on (the engine settings `reliability`). */
export interface AvailabilityBasis {
  enabled: boolean
  daysPerYear: number
  failures: { instance: number; unit: number; zone: Record<string, number>; link: number; isolation: number; geo: number }
  repair: { instance: number; unit: number; zone: Record<string, number>; link: number; isolation: number; geo: number }
  tierFactor: Record<string, number>
  isolationGroupFactor: number
  provider: { traits: string[]; byFunction: Record<string, number>; default: number; incidentSeconds: number }
  partialCapacity: boolean
  warnPercent: number
  top: number
  presets: number[]
}

/** The expected availability: how much of the year the service is down, from what fails, how often and what the architecture does about it. */
export interface Availability {
  estimate: number
  /** The service does not work even while nothing has failed: it is down all the time. */
  serviceDown: boolean
  downtimeSecondsPerYear: number
  target: number | null
  budgetSecondsPerYear: number | null
  causes: { cause: AvailabilityCause; secondsPerYear: number; share: number }[]
  units: { unit: string; name: string; secondsPerYear: number; share: number }[]
  top: AvailabilityFailure[]
  scenarios: number
  truncated: boolean
  basis: AvailabilityBasis
}

/** A number before and after a change. */
export interface Delta<T> { before: T; after: T }

/** What a change does to the design: the numbers of the report before and after, measured by evaluating the result. */
export interface Effect {
  score: Delta<number>
  critical: Delta<number>
  risk: Delta<number>
  /** Absent when the estimate does not exist on both sides. */
  availability?: Delta<number>
  downtimeSeconds?: Delta<number>
  cost: Delta<number>
  /** Open critical and risk findings (fingerprints) that are gone / came in. */
  resolved: string[]
  introduced: string[]
}

/** One change worth making: the engine's fix for a finding, and what it measurably does. */
export interface Recommendation {
  id: string
  findingId: string
  ruleId: string
  status: Status
  kind: string
  /** Placeholders of the fix's label: the finding's variables with the fix's own. */
  vars: Record<string, unknown>
  ops: Op[]
  effect: Effect
}

/** Several fixes applied one after another, each kept only when the result was better. */
export interface RecommendBundle { ids: string[]; ops: Op[]; effect: Effect }

/** "What to improve first" (`POST /recommendations`). */
export interface RecommendPlan {
  items: Recommendation[]
  bundle: RecommendBundle | null
  /** Open critical and risk findings that no fix of the engine closes: they are for a person. */
  manual: string[]
  considered: number
  truncated: boolean
}

export interface Report {
  engineVersion: number
  issues: Issue[]
  counts: Counts
  rules: RuleResult[]
  findings: Finding[]
  objectives: Objective[]
  scenarios: ScenarioSummary[]
  scenarioStats?: ScenarioStats
  baselineLoads: UnitLoad[]
  capacity: CapacityRow[]
  score: { value: number; penalty: number; coverage: number; breakdown: { ruleId: string; status: Status; penalty: number }[] }
  cost: CostReport
  /** Absent when the estimate is switched off or there is nothing to serve. */
  availability?: Availability
  units: Record<string, string>
}

/** `at`: seconds after the failure at which the step is complete (a promotion, a reroute, a restore); absent: with the failure itself. */
export interface SimEvent { code: string; unit?: string; node?: string; vars?: Record<string, unknown>; at?: number }

/** The world at one moment after the failure. The first frame holds every node and edge, the next ones only what changed. */
export interface SimFrame {
  at: number
  state: ServiceState
  capacityRatio: number | null
  nodes: Record<string, NodeState>
  edges: Record<string, EdgeState>
}
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
  /** How the picture changes while the recovery runs; absent when nothing takes time. */
  timeline?: SimFrame[]
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

/** Timings and thresholds of the editor: the `editor.*` settings of the admin panel. */
export interface EditorSettings {
  autosaveMs: number
  evaluateMs: number
  simulateMs: number
  recommendMs: number
  historySteps: number
  saveRetryMs: number
  saveRetryMaxMs: number
  jobPollMs: number
  lodCompactBelow: number
  lodMiniBelow: number
  toastMs: number
  errorToastMs: number
  undoToastMs: number
  highlightMs: number
  paletteMaxResults: number
  layoutZonesPerRow: number
  quietEdgesSites: number
  playbackStepMs: number
}

export interface Bootstrap {
  user: User
  rev: number
  version: string
  engineVersion: number
  app: { name: string; defaultLanguage: string; languages: string[]; theme: Record<string, string>; motion?: string; motionScale?: number; onboarding?: boolean; helpUrl?: string; quickPrompts: { label: LText; prompt: LText }[] }
  catalog: Catalog
  rules: Rule[]
  templates: TemplateMeta[]
  exportFormats: ExportFormatInfo[]
  /** absent when talking to a backend that predates the `editor.*` settings */
  editor?: Partial<EditorSettings>
  ai: { enabled: boolean; configured: boolean; maxPromptChars: number; plannerModel: string; fastModel: string }
  limits: { maxNodes: number; maxEdges: number; maxZones: number; waiverMinReasonChars: number; waiverMaxDays: number }
  /** Today's date on the server (`YYYY-MM-DD`): support dates are judged against it. */
  today?: string
  engine: { currency: string; symbol: string; minCapacityDuringFailure: number; headroomPercent: number; defaultFailureTolerance: string; toleranceLevels?: ToleranceLevel[]; maxSiteFailures?: number; lifecycle?: LifecycleSettings; availability?: { enabled: boolean; presets: number[]; warnPercent: number; daysPerYear: number }; recommendations?: { enabled: boolean; show: number } }
}

// ---------------------------------------------------------------------------- lifecycle

/** How the lifecycle tables are read (`engine.lifecycle` settings). */
export interface LifecycleSettings { warnDays: number; matureDays: number; preferLts: boolean; staleDays: number }

/** A date (`YYYY-MM-DD`) or a flag, the way endoflife.date reports the end of support. */
export type DateOrFlag = string | boolean

/** One release line of a product. */
export interface Cycle {
  cycle: string
  releaseDate?: string
  support?: DateOrFlag
  eol?: DateOrFlag
  latest?: string
  latestReleaseDate?: string
  lts?: boolean
}

/** The release table of one product (`catalog.versions`). */
export interface VersionSet { id: string; name?: string; source: string; asOf: string; cycles: Cycle[] }

export interface VersionsPayload { today: string; tables: VersionSet[] }

export type Phase = 'supported' | 'maintenance' | 'eol' | 'unknown'

/** What a table says about one version on one day. */
export interface Lifecycle { cycle: string; phase: Phase; eol: string | null; daysToEol: number | null; latest: string | null; lts: boolean }

export interface SyncOutcome { id: string; ok: boolean; cycles: number; asOf: string | null; error: string | null }
export interface SyncResult { results: SyncOutcome[]; ok: number; failed: number }
export interface VersionsStatus {
  today: string
  tables: number
  staleDays: number
  stale: string[]
  oldestDays: number | null
  autoSyncDays: number
  sourceBases: string[]
  last: { at: string; by: string; ok: number; failed: { id: string; error: string | null }[] } | null
}

/** One step of the failure ladder a requirement can ask for (data of the engine settings). */
export interface ToleranceLevel { id: string; rank: number; domain: ZoneLevel | null; name: LText; help: LText }

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
  /** only for the most recently changed projects (`limits.listThumbnails`); `null` = load the project */
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

// ---------------------------------------------------------------------------- placement

export interface PlacementPattern {
  id: string
  name: LText
  description?: LText | null
  pros: LText[]
  cons: LText[]
  badge?: string | null
  order: number
  minSites: number
  maxSites?: number | null
  defaultSites: number
  siteType: string
  regions: { mode: 'single' | 'count' | 'per_site'; default: number; min: number; max: number }
  racks: { default: number; min: number; max: number }
  tolerance: string
  siteFailures: { mode: string; value: number; max: number }
  witness: boolean
  standbySites: number
  syncWithinGeo: boolean
  offsiteBackupSite: boolean
  requirements: Requirements
  stack: string
}

export interface PlacementProduct { kind: string; variants: { id: string; default: boolean; managed: boolean }[] }

export interface PlacementLayer {
  id: string
  name: LText
  kind: string
  alternatives: string[]
  /** every product of this job the layer can be built from (the default one first) */
  products: PlacementProduct[]
  function?: string | null
  scope: 'global' | 'site' | 'cluster' | 'single'
  optional: boolean
  default: boolean
  /** the layer that holds the quorum of this one */
  dcs?: string | null
}

export interface PlacementOptions {
  patterns: PlacementPattern[]
  /** The pattern to offer first (the administrator's choice, else the first enabled one). */
  defaultPattern: string | null
  stacks: { id: string; name: LText; description?: LText | null; layers: PlacementLayer[] }[]
  siteTypes: string[]
  maxSiteFailures: number
  maxNodes: number
}

export interface PlacementRequest {
  pattern: string
  sites?: number
  regions?: number
  racks?: number
  siteFailures?: number
  stack?: string
  layers?: string[]
  kinds?: Record<string, string>
  /** layer id → variant of its product (a topology or a managed offering) */
  variants?: Record<string, string>
  siteType?: string
  requirements?: Requirements
  name?: string
}

export interface PlacedSite { id: string; name: string; role: 'working' | 'standby' | 'witness' | 'backup'; geo: string; racks: string[] }
export interface PlacementNote { code: string; level: 'info' | 'warn' | 'error'; vars?: Record<string, unknown> }

export interface PlacementVerdict {
  score: number
  coverage: number
  counts: Counts
  objectives: Objective[]
  cost: { total: number; currency: string; symbol: string; incomplete: number }
  scenarioStats: ScenarioStats
  worstCapacity: number | null
  findings: { id: string; ruleId: string; status: Status; vars: Record<string, unknown> }[]
}

export interface PlacementPreview {
  architecture: Architecture
  sites: PlacedSite[]
  siteFailures: number
  notes: PlacementNote[]
  report: PlacementVerdict
  nodes: number
  edges: number
}

export type PlacementVariant =
  | { ok: true; pattern: string; sites: number | null; siteFailures: number; nodes: number; zones: number; workingSites: number; arbiterSites: number; notes: PlacementNote[]; report: PlacementVerdict }
  | { ok: false; pattern: string; sites: number | null; error: { code: string; message: string } }
