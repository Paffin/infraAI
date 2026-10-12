// Rules of the users table: which controls are locked and why. The server enforces the same
// ("there must be at least one active administrator"); the UI just avoids offering a doomed click.

import type { AdminUser } from '@/api/types'

export type Protection = 'self' | 'lastAdmin' | null

export function activeAdmins(users: AdminUser[]): AdminUser[] {
  return users.filter((u) => u.role === 'admin' && !u.disabled)
}

/** Why the role and the "active" switch of `u` are locked (null = free to change). */
export function protection(u: AdminUser, currentUserId: string | undefined, admins: AdminUser[]): Protection {
  if (u.id === currentUserId) return 'self'
  if (u.role === 'admin' && !u.disabled && admins.length <= 1) return 'lastAdmin'
  return null
}

export function isLocked(u: Pick<AdminUser, 'lockedUntil'>, now: number = Date.now()): boolean {
  if (!u.lockedUntil) return false
  const ts = Date.parse(u.lockedUntil)
  return Number.isFinite(ts) && ts > now
}
