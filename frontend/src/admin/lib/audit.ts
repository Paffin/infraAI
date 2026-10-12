// Audit log helpers.

import type { AuditEvent } from '@/api/types'

/** Actions the server writes (backend/crates/server/src/api/*): used for the filter suggestions. */
export const KNOWN_ACTIONS = [
  'auth.login', 'auth.login_failed', 'auth.password_changed',
  'settings.update',
  'config.create', 'config.update', 'config.delete', 'config.reset', 'config.import',
  'user.create', 'user.update', 'user.reset_password',
  'project.create', 'project.delete',
  'ai.job', 'ai.apply',
  'waiver.create', 'waiver.revoke',
]

export type ActionTone = 'crit' | 'risk' | 'accent' | 'nodata'

export function actionTone(action: string): ActionTone {
  if (action === 'auth.login_failed') return 'crit'
  if (action.endsWith('.delete') || action.endsWith('.reset') || action.endsWith('reset_password') || action.endsWith('.import')) return 'risk'
  if (action.startsWith('settings.') || action.startsWith('config.') || action.startsWith('user.')) return 'accent'
  return 'nodata'
}

export function hasDetails(e: Pick<AuditEvent, 'details'>): boolean {
  return !!e.details && typeof e.details === 'object' && Object.keys(e.details).length > 0
}

/** Cursor of the next page ("older"): the smallest id seen so far. */
export function nextCursor(events: Pick<AuditEvent, 'id'>[]): number | undefined {
  if (!events.length) return undefined
  return events.reduce((m, e) => Math.min(m, e.id), Infinity)
}

/** Append a page without duplicating rows (a refresh may overlap with what is already shown). */
export function mergeEvents(current: AuditEvent[], page: AuditEvent[]): AuditEvent[] {
  const seen = new Set(current.map((e) => e.id))
  return [...current, ...page.filter((e) => !seen.has(e.id))]
}
