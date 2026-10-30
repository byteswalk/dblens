// 连接对话框的表单（M1 设计稿第 2 部分）：表单值与后端的连接配置（ProfileInput）互相转换，以及校验。纯逻辑，单独测试。

import type { DriverId, Environment, Profile, ProfileInput, ServerSummary } from '../../shared/api'
import { t } from '../../shared/i18n'
import { variantName } from '../editor/dialect'
import { type ProductId, baseProduct, product, productLabel, productOf } from './products'

export type TlsMode = 'disable' | 'require' | 'verify'

/** SQL Server 的认证方式：SQL 账号、指定 Windows 账号、当前 Windows 用户（服务端进程的身份）。 */
export type SqlServerAuth = 'sql' | 'windows' | 'integrated'

export interface ConnectionForm {
  name: string
  /** 选的产品；驱动由产品决定。 */
  product: ProductId
  driver: DriverId
  host: string
  port: number
  user: string
  password: string
  /** 保存密码（存系统钥匙串，或主密码加密的本地密码库）。 */
  savePassword: boolean
  /** MySQL 系、PG 系：默认库；Oracle：服务名；达梦：登录后切换到的模式。 */
  database: string
  /** 只用于 Oracle：用 SID 而不是服务名连接。 */
  oracleSid: string
  environment: Environment
  color: string
  tls: TlsMode
  tlsCa: string
  /** 只用于 SQL Server：认证方式、命名实例、信任服务器证书。 */
  sqlAuth: SqlServerAuth
  instance: string
  trustCert: boolean
  timeoutSecs: number
  /** 其他驱动参数，一行一个 `键=值`。 */
  extra: string
}

/** 新建连接时可选的驱动，按界面上的顺序。 */
export const DRIVER_IDS: DriverId[] = ['mysql', 'postgres', 'oracle', 'dameng', 'sqlserver']

/** 驱动的显示名称（如「MySQL 系」）。 */
export function driverLabel(driver: DriverId): string {
  return t(`connections.driver.${driver}.label`)
}

/** 驱动的说明（包含哪些兼容库、需要什么客户端库）。 */
export function driverNote(driver: DriverId): string {
  return t(`connections.driver.${driver}.note`)
}

export const DEFAULT_PORTS: Record<DriverId, number> = { mysql: 3306, postgres: 5432, oracle: 1521, dameng: 5236, sqlserver: 1433 }

/** 「数据库」一栏在各驱动下的含义。 */
export function databaseLabel(driver: DriverId): string {
  return t(`connections.databaseLabel.${driver}`)
}

/** MySQL 系、PG 系（M0-4b）和 SQL Server 支持 TLS 选项。 */
export function supportsTls(driver: DriverId): boolean {
  return driver === 'mysql' || driver === 'postgres' || driver === 'sqlserver'
}

/** 当前 Windows 用户认证不需要填用户名和密码。 */
export function needsCredentials(f: Pick<ConnectionForm, 'driver' | 'sqlAuth'>): boolean {
  return !(f.driver === 'sqlserver' && f.sqlAuth === 'integrated')
}

export function emptyForm(id: ProductId = 'mysql'): ConnectionForm {
  const p = product(id)
  return {
    name: '',
    product: id,
    driver: p.driver,
    host: '127.0.0.1',
    port: p.port,
    user: '',
    password: '',
    savePassword: true,
    database: '',
    oracleSid: '',
    environment: 'none',
    color: '',
    tls: 'disable',
    tlsCa: '',
    sqlAuth: 'sql',
    instance: '',
    trustCert: false,
    timeoutSecs: 10,
    extra: '',
  }
}

/** 换驱动时：端口还是上一个驱动的默认值，就换成新驱动的默认值；不支持 TLS 的驱动关掉 TLS。 */
export function switchDriver(f: ConnectionForm, driver: DriverId): ConnectionForm {
  const port = f.port === DEFAULT_PORTS[f.driver] ? DEFAULT_PORTS[driver] : f.port
  return { ...f, driver, port, tls: supportsTls(driver) ? f.tls : 'disable', tlsCa: supportsTls(driver) ? f.tlsCa : '' }
}

/** 换产品：驱动跟着换；端口还是上一个产品的默认值，就换成新产品的默认值。 */
export function switchProduct(f: ConnectionForm, id: ProductId): ConnectionForm {
  const next = product(id)
  const port = f.port === product(f.product).port ? next.port : f.port
  return { ...switchDriver(f, next.driver), product: id, port }
}

const KNOWN_OPTIONS = ['tls', 'tls_ca', 'sid', 'auth', 'instance', 'trust_server_certificate']

export function fromProfile(p: Profile, hasPassword: boolean): ConnectionForm {
  const opts = p.options ?? {}
  const extra = Object.entries(opts)
    .filter(([k]) => !KNOWN_OPTIONS.includes(k))
    .map(([k, v]) => `${k}=${v}`)
    .join('\n')
  const tls = opts.tls === 'require' || opts.tls === 'verify' ? opts.tls : 'disable'
  return {
    name: p.name,
    product: productOf(p.driver, p.product),
    driver: p.driver,
    host: p.host,
    port: p.port,
    user: p.user,
    password: '',
    savePassword: hasPassword,
    database: p.database ?? '',
    oracleSid: opts.sid ?? '',
    environment: p.environment ?? 'none',
    color: p.color ?? '',
    tls,
    tlsCa: opts.tls_ca ?? '',
    sqlAuth: opts.auth === 'windows' || opts.auth === 'integrated' ? opts.auth : 'sql',
    instance: opts.instance ?? '',
    trustCert: opts.trust_server_certificate === 'true',
    timeoutSecs: p.connect_timeout_secs ?? 10,
    extra,
  }
}

export function toProfileInput(f: ConnectionForm): ProfileInput {
  const options: Record<string, string> = {}
  for (const line of f.extra.split('\n')) {
    const i = line.indexOf('=')
    if (i > 0) options[line.slice(0, i).trim()] = line.slice(i + 1).trim()
  }
  if (supportsTls(f.driver) && f.tls !== 'disable') {
    options.tls = f.tls
    if (f.tls === 'verify' && f.tlsCa.trim()) options.tls_ca = f.tlsCa.trim()
  }
  if (f.driver === 'oracle' && f.oracleSid.trim()) options.sid = f.oracleSid.trim()
  if (f.driver === 'sqlserver') {
    if (f.sqlAuth !== 'sql') options.auth = f.sqlAuth
    if (f.instance.trim()) options.instance = f.instance.trim()
    if (f.trustCert) options.trust_server_certificate = 'true'
  }
  return {
    name: f.name.trim(),
    driver: f.driver,
    host: f.host.trim(),
    port: f.port,
    user: f.user.trim(),
    database: f.database.trim() || null,
    options,
    environment: f.environment,
    color: f.color.trim() || null,
    connect_timeout_secs: f.timeoutSecs,
    product: f.product,
  }
}

/** 校验，返回 字段 → 错误说明；没有错误时为空对象。 */
export function validate(f: ConnectionForm): Partial<Record<keyof ConnectionForm, string>> {
  const e: Partial<Record<keyof ConnectionForm, string>> = {}
  if (!f.name.trim()) e.name = t('connections.validate.nameRequired')
  if (!f.host.trim()) e.host = t('connections.validate.hostRequired')
  if (!Number.isInteger(f.port) || f.port < 1 || f.port > 65535) e.port = t('connections.validate.portRange')
  if (needsCredentials(f) && !f.user.trim()) e.user = t('connections.validate.userRequired')
  if (f.driver === 'oracle' && !f.database.trim() && !f.oracleSid.trim()) {
    e.database = t('connections.validate.serviceOrSidRequired')
  }
  if (!Number.isInteger(f.timeoutSecs) || f.timeoutSecs < 1 || f.timeoutSecs > 600) {
    e.timeoutSecs = t('connections.validate.timeoutRange')
  }
  for (const line of f.extra.split('\n')) {
    if (line.trim() && line.indexOf('=') <= 0) {
      e.extra = t('connections.validate.extraBadLine', { line: line.trim() })
      break
    }
  }
  return e
}

/** 测试连接成功后显示的服务器版本：有的数据库（如 PostgreSQL）返回的版本字符串已经带产品名，不再重复。 */
export function serverText(s: ServerSummary): string {
  return s.version.startsWith(s.product) ? s.version : `${s.product} ${s.version}`
}

/** 连接列表、状态栏中的数据库名称和图标：连接过时按识别出的变体；没连接过时按新建时选的产品，没有选或选的是标准库时按驱动。 */
export function profileKind(p: Pick<Profile, 'driver' | 'variant' | 'product'>): { label: string; icon: string | null } {
  if (p.variant) return { label: variantName(p.variant) ?? driverLabel(p.driver), icon: p.variant }
  const id = productOf(p.driver, p.product)
  return id === baseProduct(p.driver) ? { label: driverLabel(p.driver), icon: null } : { label: productLabel(id), icon: id }
}

/** 测试连接成功后另外显示的说明：兼容模式、未适配的模式、所选产品与识别结果不一致。 */
export function serverNotes(s: ServerSummary, chosen: ProductId): { text: string; warn: boolean }[] {
  const out: { text: string; warn: boolean }[] = []
  if (s.mode) {
    const key = s.unadapted_mode ? 'connections.dialog.modeUnadapted' : s.compat === 'oracle' ? 'connections.dialog.modeOracle' : 'connections.dialog.modeNative'
    out.push({ text: t(key, { mode: s.mode }), warn: s.unadapted_mode })
  }
  const detected = productOf(product(chosen).driver, s.variant)
  // 选的是标准库（如 PostgreSQL）而识别出兼容库时不算不一致：较早的连接都按驱动选的
  if (chosen !== detected && chosen !== product(chosen).driver) {
    out.push({ text: t('connections.dialog.productMismatch', { chosen: productLabel(chosen), detected: s.variant_name }), warn: true })
  }
  return out
}
