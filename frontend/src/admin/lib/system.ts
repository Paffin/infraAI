// Presentation helpers for GET /admin/system.

export type { SystemInfo } from '@/api/types'

/** "PostgreSQL 16.15 (Ubuntu 16.15-0ubuntu0.24.04.1) on x86_64..." -> "PostgreSQL 16.15" */
export function shortDbVersion(v: string | null | undefined): string {
  if (!v) return '—'
  const m = /^(\S+\s+[\d.]+)/.exec(v)
  return m ? m[1] : v.slice(0, 40)
}

/** 93784 -> { d: 1, h: 2, m: 3 } */
export function splitUptime(seconds: number): { d: number; h: number; m: number } {
  const s = Math.max(0, Math.floor(seconds))
  return { d: Math.floor(s / 86400), h: Math.floor((s % 86400) / 3600), m: Math.floor((s % 3600) / 60) }
}

const JOB_ORDER = ['queued', 'running', 'succeeded', 'failed', 'cancelled']

export function sortJobs<T extends { status: string }>(jobs: T[]): T[] {
  const rank = (s: string) => (JOB_ORDER.includes(s) ? JOB_ORDER.indexOf(s) : JOB_ORDER.length)
  return [...jobs].sort((a, b) => rank(a.status) - rank(b.status))
}

export function jobTone(status: string): 'ok' | 'crit' | 'risk' | 'accent' | 'nodata' {
  switch (status) {
    case 'succeeded': return 'ok'
    case 'failed': return 'crit'
    case 'running': return 'accent'
    case 'queued': return 'risk'
    default: return 'nodata'
  }
}
