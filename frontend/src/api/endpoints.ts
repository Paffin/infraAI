import { http, qs } from './client'
import type {
  AdminUser, AiJob, ArchDiff, Architecture, AuditEvent, Bootstrap, ChatMessage, ConfigError, ConfigExport, ConfigImportResult,
  ConfigItem, ConfigKindCounter, ExportFile, IacReport, LlmModel, LlmTestResult, LlmUsage, Op, ProjectDetail, ProjectListItem, Proposal,
  PlacementOptions, PlacementPreview, PlacementRequest, PlacementVariant, Report, ReportSummary, Scenario, SettingDef, SimResult, SystemInfo, Template, User, VersionItem,
} from './types'

export const auth = {
  login: (email: string, password: string) => http.post<{ user: User }>('/auth/login', { email, password }),
  logout: () => http.post<{ ok: boolean }>('/auth/logout'),
  me: () => http.get<{ user: User }>('/auth/me'),
  changePassword: (current: string, next: string) => http.post<{ ok: boolean }>('/auth/password', { current, new: next }),
  publicConfig: () => http.get<{ appName: string; defaultLanguage: string; languages: string[]; theme: Record<string, string> }>('/public/config'),
}

export const bootstrap = () => http.get<Bootstrap>('/bootstrap')

export const projects = {
  list: () => http.get<{ projects: ProjectListItem[] }>('/projects'),
  create: (b: { name: string; description?: string; templateId?: string; architecture?: Architecture; placement?: PlacementRequest; lang?: string }) => http.post<{ id: string }>('/projects', b),
  get: (id: string) => http.get<ProjectDetail>(`/projects/${id}`),
  patch: (id: string, b: { name?: string; description?: string }) => http.patch<{ ok: boolean }>(`/projects/${id}`, b),
  remove: (id: string) => http.del<{ ok: boolean }>(`/projects/${id}`),
  saveDraft: (id: string, architecture: Architecture, expectedRev: number) => http.put<{ rev: number }>(`/projects/${id}/draft`, { architecture, expectedRev }),
  versions: (id: string) => http.get<{ versions: VersionItem[] }>(`/projects/${id}/versions`),
  createVersion: (id: string, message: string, lang?: string) => http.post<{ n: number }>(`/projects/${id}/versions`, { message, lang }),
  version: (id: string, n: number) => http.get<{ n: number; message: string; author: string; architecture: Architecture; summary: ReportSummary; createdAt: string }>(`/projects/${id}/versions/${n}`),
  restore: (id: string, n: number, expectedRev: number) => http.post<{ rev: number }>(`/projects/${id}/versions/${n}/restore`, { expectedRev }),
  diff: (id: string, from: string | number, to: string | number) => http.get<ArchDiff>(`/projects/${id}/diff${qs({ from, to })}`),
  createWaiver: (id: string, b: { fingerprint: string; reason: string; expiresInDays?: number; lang?: string }) => http.post<{ id: string }>(`/projects/${id}/waivers`, b),
  revokeWaiver: (id: string, wid: string) => http.del<{ ok: boolean }>(`/projects/${id}/waivers/${wid}`),
  export: (id: string, format: string, lang?: string) => http.get<{ format: string; files: ExportFile[]; report: IacReport | null }>(`/projects/${id}/export/${format}${qs({ lang })}`),
  exportUrl: (id: string, format: string, lang?: string) => `/api/projects/${id}/export/${format}${qs({ lang, download: 1 })}`,
}

export const engine = {
  evaluate: (architecture: Architecture, projectId?: string, lang?: string, signal?: AbortSignal) => http.post<Report>('/evaluate', { architecture, projectId, lang }, signal),
  simulate: (architecture: Architecture, scenario: Scenario) => http.post<SimResult>('/simulate', { architecture, scenario }),
  apply: (architecture: Architecture, ops: Op[], opts: { preview?: boolean; projectId?: string; lang?: string } = {}) =>
    http.post<{ architecture: Architecture; cascadedEdges: string[]; renamed: Record<string, string>; diff?: ArchDiff; before?: ReportSummary; after?: ReportSummary; introduced?: string[]; resolved?: string[] }>('/ops/apply', { architecture, ops, ...opts }),
  template: (id: string) => http.get<Template>(`/templates/${id}`),
  diff: (from: Architecture, to: Architecture) => http.post<ArchDiff>('/diff', { from, to }),
}

export const placement = {
  options: () => http.get<PlacementOptions>('/placement/options'),
  preview: (req: PlacementRequest, lang?: string, signal?: AbortSignal) => http.post<PlacementPreview>('/placement/preview', { ...req, lang }, signal),
  compare: (base: PlacementRequest, variants: Partial<PlacementRequest>[], lang?: string, signal?: AbortSignal) => http.post<{ variants: PlacementVariant[] }>('/placement/compare', { base: { ...base, lang }, variants }, signal),
}

export const ai = {
  createJob: (projectId: string, b: { kind: 'generate' | 'patch' | 'fix' | 'chat'; prompt?: string; findingId?: string; lang?: string }) => http.post<{ jobId: string }>(`/projects/${projectId}/ai/jobs`, b),
  job: (id: string) => http.get<AiJob>(`/ai/jobs/${id}`),
  cancel: (id: string) => http.post<{ ok: boolean }>(`/ai/jobs/${id}/cancel`),
  chat: (projectId: string, after?: number) => http.get<{ messages: ChatMessage[]; runningJobId: string | null }>(`/projects/${projectId}/chat${qs({ after })}`),
  clearChat: (projectId: string) => http.del<{ ok: boolean }>(`/projects/${projectId}/chat`),
  proposal: (id: string) => http.get<Proposal>(`/proposals/${id}`),
  apply: (id: string, expectedRev?: number) => http.post<{ rev: number; architecture: Architecture; version: number }>(`/proposals/${id}/apply`, { expectedRev }),
  reject: (id: string) => http.post<{ ok: boolean }>(`/proposals/${id}/reject`),
}

export const admin = {
  settings: () => http.get<{ settings: SettingDef[]; rev: number }>('/admin/settings'),
  saveSettings: (changes: Record<string, unknown>) => http.put<{ settings: SettingDef[]; rev: number }>('/admin/settings', { changes }),
  models: (p: { q?: string; structured?: boolean; refresh?: boolean; limit?: number } = {}) =>
    http.get<{ models: LlmModel[]; total: number }>(`/admin/llm/models${qs({ q: p.q, structured: p.structured ? 1 : undefined, refresh: p.refresh ? 1 : undefined, limit: p.limit })}`),
  testLlm: (ping = false) => http.post<LlmTestResult>('/admin/llm/test', { ping }),
  usage: (days = 30) => http.get<LlmUsage>(`/admin/llm/usage${qs({ days })}`),
  configKinds: () => http.get<{ kinds: ConfigKindCounter[]; errors: ConfigError[] }>('/admin/config'),
  configList: (kind: string) => http.get<{ items: ConfigItem[] }>(`/admin/config/${kind}`),
  configCreate: (kind: string, id: string, data: unknown) => http.post<{ ok: boolean }>(`/admin/config/${kind}`, { id, data }),
  configPut: (kind: string, id: string, data: unknown) => http.put<{ ok: boolean }>(`/admin/config/${kind}/${encodeURIComponent(id)}`, { data }),
  configDelete: (kind: string, id: string) => http.del<{ ok: boolean }>(`/admin/config/${kind}/${encodeURIComponent(id)}`),
  configReset: (kind: string, id: string) => http.post<{ ok: boolean }>(`/admin/config/${kind}/${encodeURIComponent(id)}/reset`),
  configExport: () => http.get<ConfigExport>('/admin/config-export'),
  configImport: (b: { settings?: Record<string, unknown>; items?: unknown[]; dryRun?: boolean }) => http.post<ConfigImportResult>('/admin/config-import', b),
  users: () => http.get<{ users: AdminUser[] }>('/admin/users'),
  createUser: (b: { email: string; name?: string; role: string; password?: string }) => http.post<{ id: string; temporaryPassword?: string }>('/admin/users', b),
  patchUser: (id: string, b: { name?: string; role?: string; disabled?: boolean }) => http.patch<{ ok: boolean }>(`/admin/users/${id}`, b),
  resetPassword: (id: string, password?: string) => http.post<{ temporaryPassword?: string }>(`/admin/users/${id}/reset-password`, { password }),
  audit: (p: { limit?: number; before?: number; action?: string; actor?: string } = {}) => http.get<{ events: AuditEvent[] }>(`/admin/audit${qs(p)}`),
  system: () => http.get<SystemInfo>('/admin/system'),
}
