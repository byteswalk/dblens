// REST 接口客户端。接口说明见 docs/notes/m0-4b-api.md；页面与接口同源，路径都以 /api 开头。

export type DriverId = 'mysql' | 'postgres' | 'oracle' | 'dameng'
export type Environment = 'none' | 'dev' | 'test' | 'prod'

/** 服务端的错误对象（dblens_core::Error），按 kind 区分。 */
export interface ErrorBody {
  kind: string
  message?: string
  code?: string
  position?: number | null
  what?: string
  reason?: string
  hint?: string
}

export class ApiError extends Error {
  readonly status: number
  readonly body: ErrorBody

  constructor(status: number, body: ErrorBody) {
    super(errorText(body))
    this.status = status
    this.body = body
  }
}

/** 把错误对象转成给用户看的一句话，与服务端 Display 的措辞一致。 */
export function errorText(e: ErrorBody): string {
  switch (e.kind) {
    case 'connect':
      return `连接失败：${e.message}`
    case 'auth':
      return `认证失败：${e.message}`
    case 'sql':
      return `SQL 错误 [${e.code}]：${e.message}`
    case 'timeout':
      return '执行超时'
    case 'cancelled':
      return '执行已取消'
    case 'driver_unavailable':
      return `驱动不可用：${e.hint}`
    case 'invalid':
      return `无效的输入：${e.message}`
    case 'not_found':
      return `找不到${e.what}`
    case 'busy':
      return `会话正忙：${e.message}`
    case 'locked':
      return '密码库已锁定，请先输入主密码'
    case 'denied':
      return `执行被拒绝：${e.reason}`
    case 'internal':
      return `内部错误：${e.message}`
    default:
      return JSON.stringify(e)
  }
}

export interface DriverStatus {
  id: DriverId
  name: string
  compiled: boolean
  available: boolean
  needs_native_lib: boolean
  lib_dir: string | null
  suggested_lib_dir: string | null
  hint: string | null
}

export interface ProfileInput {
  name: string
  driver: DriverId
  host: string
  port: number
  user: string
  database?: string | null
  options?: Record<string, string>
  environment?: Environment
  color?: string | null
  connect_timeout_secs?: number
}

export interface Profile extends ProfileInput {
  id: string
}

export interface ServerSummary {
  product: string
  version: string
  variant: string
  variant_name: string
  version_code: number | null
}

export interface SecretStatus {
  backend: 'keyring' | 'vault'
  vault_initialized: boolean
  unlocked: boolean
}

export interface SessionInfo {
  id: number
  profile_id: string
  variant: string
  autocommit: boolean
  in_transaction: boolean
  busy: boolean
  broken: boolean
}

export type FetchFn = (input: string, init?: RequestInit) => Promise<Response>

export function createApi(token: string, fetchFn: FetchFn = (i, o) => fetch(i, o)) {
  async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
    const headers: Record<string, string> = { Authorization: `Bearer ${token}` }
    if (body !== undefined) headers['Content-Type'] = 'application/json'
    const resp = await fetchFn(`/api${path}`, {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
    })
    const text = await resp.text()
    const data = text ? JSON.parse(text) : null
    if (!resp.ok) {
      throw new ApiError(resp.status, data ?? { kind: 'internal', message: `HTTP ${resp.status}` })
    }
    return data as T
  }

  return {
    drivers: () => request<DriverStatus[]>('GET', '/drivers'),
    setLibDir: (driver: DriverId, path: string | null) =>
      request<DriverStatus>('PUT', `/drivers/${driver}/lib-dir`, { path }),
    secrets: () => request<SecretStatus>('GET', '/secrets'),
    profiles: () => request<Profile[]>('GET', '/profiles'),
    createProfile: (profile: ProfileInput, password: string | null) =>
      request<Profile>('POST', '/profiles', { profile, password }),
    deleteProfile: (id: string) => request<null>('DELETE', `/profiles/${encodeURIComponent(id)}`),
    testConnection: (profile: ProfileInput, password: string) =>
      request<ServerSummary>('POST', '/profiles/test', { profile, password }),
    openSession: (profileId: string) => request<SessionInfo>('POST', '/sessions', { profile_id: profileId }),
    closeSession: (id: number) => request<null>('DELETE', `/sessions/${id}?commit=false`),
    /** 会话的 WebSocket 地址。浏览器的 WebSocket 不能带请求头，令牌放在查询参数里。 */
    wsUrl: (sessionId: number, loc: { protocol: string; host: string } = window.location) =>
      `${loc.protocol === 'https:' ? 'wss' : 'ws'}://${loc.host}/api/sessions/${sessionId}/ws?token=${encodeURIComponent(token)}`,
  }
}

export type Api = ReturnType<typeof createApi>
