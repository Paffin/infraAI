// Thin typed fetch wrapper. Cookies carry the session; state-changing requests rely on SameSite +
// the server-side Origin check, so no token juggling is needed here.

export class ApiError extends Error {
  status: number
  code: string
  details?: any
  /** Seconds the server asks to wait before the same request is sent again (`Retry-After`; the server writes whole seconds). Only on the refusals that are about time. */
  retryAfter?: number
  constructor(status: number, code: string, message: string, details?: any, retryAfter?: number) {
    super(message)
    this.status = status
    this.code = code
    this.details = details
    this.retryAfter = retryAfter
  }
}

export const unauthorizedEvent = 'ia:unauthorized'

async function request<T>(method: string, path: string, body?: unknown, signal?: AbortSignal): Promise<T> {
  let res: Response
  try {
    res = await fetch('/api' + path, {
      method,
      headers: body !== undefined ? { 'Content-Type': 'application/json' } : undefined,
      body: body !== undefined ? JSON.stringify(body) : undefined,
      credentials: 'same-origin',
      signal,
    })
  } catch (e: any) {
    if (e?.name === 'AbortError') throw e
    throw new ApiError(0, 'network', e?.message || 'network error')
  }
  if (res.status === 304) return undefined as T
  const text = await res.text()
  let data: any = undefined
  try {
    data = text ? JSON.parse(text) : undefined
  } catch {
    data = text
  }
  if (!res.ok) {
    const err = data?.error
    if (res.status === 401 && path !== '/auth/login') window.dispatchEvent(new Event(unauthorizedEvent))
    const wait = Number(res.headers.get('Retry-After'))
    throw new ApiError(res.status, err?.code || 'http_' + res.status, err?.message || res.statusText, err?.details, Number.isFinite(wait) && wait > 0 ? wait : undefined)
  }
  return data as T
}

export const http = {
  get: <T>(p: string, signal?: AbortSignal) => request<T>('GET', p, undefined, signal),
  post: <T>(p: string, b?: unknown, signal?: AbortSignal) => request<T>('POST', p, b ?? {}, signal),
  put: <T>(p: string, b: unknown) => request<T>('PUT', p, b),
  patch: <T>(p: string, b: unknown) => request<T>('PATCH', p, b),
  del: <T>(p: string) => request<T>('DELETE', p),
}

export function qs(params: Record<string, string | number | boolean | undefined | null>): string {
  const u = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) if (v !== undefined && v !== null && v !== '') u.set(k, String(v))
  const s = u.toString()
  return s ? '?' + s : ''
}
