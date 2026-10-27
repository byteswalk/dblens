// REST 接口客户端。接口说明见 docs/notes/m0-4b-api.md；页面与接口同源，路径都以 /api 开头。
// 请求都带上当前界面语言（REST 用 Accept-Language，WebSocket 用 lang 参数），后端按它输出错误和提示。

import type { CellValue } from './exec'
import { currentLang, t } from './i18n'

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

/** 把错误对象转成给用户看的一句话，与服务端 Display 的措辞一致（细节文字已由服务端按当前语言给出）。 */
export function errorText(e: ErrorBody): string {
  const v = { message: e.message ?? '', code: e.code ?? '', hint: e.hint ?? '', what: e.what ?? '', reason: e.reason ?? '' }
  switch (e.kind) {
    case 'connect':
      return t('common.error.connect', v)
    case 'auth':
      return t('common.error.auth', v)
    case 'sql':
      return t('common.error.sql', v)
    case 'timeout':
      return t('common.error.timeout')
    case 'cancelled':
      return t('common.error.cancelled')
    case 'driver_unavailable':
      return t('common.error.driver_unavailable', v)
    case 'invalid':
      return t('common.error.invalid', v)
    case 'not_found':
      return t('common.error.not_found', v)
    case 'busy':
      return t('common.error.busy', v)
    case 'locked':
      return t('common.error.locked')
    case 'denied':
      return t('common.error.denied', v)
    case 'internal':
      return t('common.error.internal', v)
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
  /** 上次连接成功时识别出的数据库变体（如 kingbase），没连接过时为空。 */
  variant?: string | null
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
  /** 当前 schema（MySQL 系为当前库）；读不到时为 null。 */
  schema: string | null
}

/** schema 列表中的一项。 */
export interface SchemaInfo {
  name: string
  /** 数据库自带的系统 schema，界面默认折叠到「系统」分组。 */
  is_system: boolean
}

/** 对象树里的表、视图、物化视图节点。 */
export interface ObjectInfo {
  name: string
  comment: string | null
}

export type ObjectKind = 'tables' | 'views' | 'materialized_views'

export interface ColumnInfo {
  name: string
  ordinal: number
  data_type: { native: string; logical: unknown }
  nullable: boolean
  default: string | null
  comment: string | null
  auto_increment?: boolean
  generated?: string | null
}

/** 表明细（只列出对象树用到的字段）。 */
export interface TableInfo {
  name: string
  comment: string | null
  columns: ColumnInfo[]
  primary_key: { name: string | null; columns: string[] } | null
  uniques?: { name: string | null; columns: string[] }[]
}

/** 查看 DDL 的对象种类（M1c2）。 */
export type DdlKind =
  | 'table'
  | 'view'
  | 'materialized_view'
  | 'sequence'
  | 'function'
  | 'procedure'
  | 'package'
  | 'package_body'
  | 'synonym'

/** 生成 SQL、数据标签的对象种类：视图和物化视图只能生成 SELECT。 */
export type TargetKind = 'table' | 'view' | 'materialized_view'

export type GenerateKind = 'select' | 'insert' | 'update' | 'delete' | 'join'

/** 表的一个外键关系：out 为本表引用的表，in 为引用本表的表（同一 schema）。 */
export interface Relation {
  direction: 'out' | 'in'
  name: string | null
  schema: string
  table: string
  local_columns: string[]
  other_columns: string[]
}

export interface GenerateParams {
  object: string
  objectKind: TargetKind
  kind: GenerateKind
  /** 关联查询只连接这张表；不给时连接本表引用的全部表。 */
  ref?: { schema: string; table: string }
}

export interface DataSqlParams {
  object: string
  filter?: string
  order?: { column: string; desc: boolean } | null
  count?: boolean
}

/** 导出格式（M1c3）。 */
export type ExportFormat = 'csv' | 'xlsx' | 'insert' | 'json'

/** 导出选项；只有对应格式用到的字段起作用。 */
export interface ExportOptions {
  delimiter: ',' | '\t' | ';'
  header: boolean
  bom: boolean
  null_text: string
  table: string
  rows_per_insert: number
}

/** 导出什么：一条查询语句，或数据标签的对象。 */
export type ExportSource =
  | { sql: string }
  | { data: { schema: string; object: string; filter?: string; order?: { column: string; desc: boolean } | null } }

export interface ExportRequest {
  profile_id: string
  source: ExportSource
  format: ExportFormat
  options: ExportOptions
  /** 桌面版"另存为"选出的路径；浏览器模式不给。 */
  path: string | null
  file_name: string
}

export interface ExportStatus {
  id: string
  state: 'running' | 'done' | 'failed' | 'cancelled'
  rows: number
  warnings: { lob_truncated: number; cell_truncated: number }
  error: ErrorBody | null
  file_name: string
  path: string | null
}

/** 表数据编辑的一个改动（M2a）；值的格式同结果集。 */
export type RowChange =
  | { kind: 'update'; key: Record<string, CellValue>; original: Record<string, CellValue>; values: Record<string, CellValue> }
  | { kind: 'insert'; values: Record<string, CellValue> }
  | { kind: 'delete'; key: Record<string, CellValue> }

export interface ChangeSet {
  schema: string
  table: string
  changes: RowChange[]
}

/** 改动集的执行结果：失败时已整体回滚，`failed.index` 是失败的改动序号。 */
export interface ApplyOutcome {
  applied: number
  failed: { index: number; error: ErrorBody } | null
}

/** 一条运维查询（M1c3）。 */
export interface OpsQuery {
  key: string
  name: string
  category: 'session' | 'lock' | 'storage' | 'sql'
  sql: string
}

export interface SequenceInfo {
  name: string
}

export interface RoutineInfo {
  name: string
  kind: 'procedure' | 'function' | 'package' | 'package_body'
}

export interface SynonymInfo {
  name: string
  target_schema: string | null
  target_name: string
  db_link: string | null
}

/** 一条执行历史（M1c1）。 */
export interface HistoryItem {
  id: number
  /** 执行开始时间，Unix 毫秒。 */
  at: number
  profile_id: string
  /** 执行时的连接名（连接删除后仍能显示）。 */
  profile_name: string
  driver: DriverId
  variant: string | null
  schema: string | null
  sql: string
  elapsed_ms: number | null
  /** 查询取回的行数，或增删改影响的行数。 */
  rows: number | null
  /** 失败、被拒绝或被取消时的错误文字；成功时为 null。 */
  error: string | null
}

export interface HistoryParams {
  q?: string
  profile?: string
  failed?: boolean
  /** 上一页最后一条的 id。 */
  before?: number
  limit?: number
}

export type FavoriteKind = 'folder' | 'item'

/** 收藏的文件夹或 SQL（M1c1）。 */
export interface Favorite {
  id: string
  parent_id: string | null
  kind: FavoriteKind
  name: string
  sql: string | null
  note: string | null
  driver: DriverId | null
  variant: string | null
  /** 同一文件夹内的次序。 */
  position: number
  created_at: number
  updated_at: number
}

export interface NewFavorite {
  parent_id: string | null
  kind: FavoriteKind
  name: string
  sql?: string
  note?: string
  driver?: DriverId
  variant?: string | null
}

export interface FavoritePatch {
  name?: string
  sql?: string
  /** 空字符串清除说明。 */
  note?: string
  move?: { parent_id: string | null; position: number }
}

/** 逻辑类型（dblens_core::LogicalType 的 JSON，按 kind 区分）。 */
export type LogicalType =
  | { kind: 'bool' | 'text' | 'blob' | 'date' | 'interval' | 'json' | 'uuid' | 'other' }
  | { kind: 'int' | 'u_int' | 'float'; bits: number }
  | { kind: 'decimal'; precision: number | null; scale: number | null }
  | { kind: 'char' | 'var_char' | 'binary' | 'var_binary'; len: number | null }
  | { kind: 'time'; precision: number | null }
  | { kind: 'timestamp'; tz: boolean; precision: number | null }

/** 导入、执行 SQL 文件的文件来源（M2b）：上传的文件，或桌面版的本机路径。 */
export type FileSource = { upload: string } | { path: string }

export interface UploadInfo {
  id: string
  name: string
  size: number
}

export type TextEncoding = 'auto' | 'utf8' | 'gbk'

export interface FileOptions {
  format: 'csv' | 'excel'
  encoding?: TextEncoding
  /** CSV 的分隔符；不给时自动判断。 */
  delimiter?: string | null
  header?: boolean
  sheet?: string | null
}

export interface FilePreview {
  columns: string[]
  rows: CellValue[][]
  /** 按前 1000 行推断的类型。 */
  types: LogicalType[]
  warnings: { ragged_rows: number; bad_encoding: number; error_cells: number }
  sheets: string[]
  encoding: string | null
  delimiter: string | null
}

export interface NewTable {
  columns: { name: string; logical: LogicalType; nullable: boolean }[]
  primary_key: string[]
}

export type WriteMode = 'append' | 'replace' | 'upsert'

export interface ImportRequest {
  profile_id: string
  source: FileSource
  file: FileOptions
  schema: string
  table: string
  /** 新建表；不给时导入到已有表。 */
  create: NewTable | null
  /** 文件第 source 列（从 0 开始）写入表的 target 列。 */
  mapping: { source: number; target: string }[]
  mode: WriteMode
  transaction: 'whole' | 'batch'
  batch_size: number
  on_error: 'stop' | 'skip'
  empty_as_string: boolean
}

export interface SqlFileRequest {
  profile_id: string
  source: FileSource
  encoding: TextEncoding
  schema: string | null
  continue_on_error: boolean
  transaction: boolean
}

export interface TaskStatus {
  id: string
  kind: 'import' | 'sql_file' | 'design'
  state: 'running' | 'confirm' | 'done' | 'failed' | 'cancelled'
  phase: 'preparing' | 'scanning' | 'creating' | 'clearing' | 'writing' | 'executing' | 'finished'
  progress: {
    read: number
    total: number
    rows_read: number
    rows_written: number
    rows_skipped: number
    statements_done: number
    statements_total: number
    statements_failed: number
    affected: number
  }
  warnings: number
  error: ErrorBody | null
  error_line: number | null
  committed_line: number | null
  failures: { index: number; line: number; error: ErrorBody; text: string }[]
  confirm: { items: { line: number; text: string; reason: string }[]; more: number } | null
  report: string | null
  /** 表设计器：失败后没有执行的语句。 */
  remaining: string | null
}

/** 表设计器（M3）：与后端结构模型一致的完整表结构。 */
export interface DesignColumn {
  name: string
  ordinal: number
  data_type: { native: string; logical: unknown }
  nullable: boolean
  default: string | null
  comment: string | null
  auto_increment: boolean
  generated: string | null
}

export interface DesignKey {
  name: string | null
  columns: string[]
}

export interface DesignCheck {
  name: string | null
  expression: string
}

export interface DesignForeignKey {
  name: string | null
  columns: string[]
  ref_schema: string | null
  ref_table: string
  ref_columns: string[]
  on_delete: string | null
  on_update: string | null
}

export interface DesignIndex {
  name: string
  columns: { name: string; descending: boolean }[]
  unique: boolean
  method: string | null
}

export interface DesignTableModel {
  name: string
  comment: string | null
  columns: DesignColumn[]
  primary_key: DesignKey | null
  uniques: DesignKey[]
  foreign_keys: DesignForeignKey[]
  checks: DesignCheck[]
  indexes: DesignIndex[]
  triggers: unknown[]
  partition: string | null
}

/** 类型的参数形式：无参数、长度（也用于时间类型的小数秒精度）、精度加小数位。 */
export type TypeParams = 'none' | 'length' | 'precision_scale'

export interface TypeEntry {
  name: string
  params: TypeParams
  /** 长度可选按 BYTE 还是 CHAR 计（Oracle、达梦的字符类型）。 */
  char_unit: boolean
}

/** 方言能力：前端据此禁用库不支持的选项。 */
export interface DesignCaps {
  reorder: boolean
  checks: boolean
  descending_index: boolean
  on_delete: string[]
  on_update: string[]
  index_methods: string[]
  types: TypeEntry[]
}

export interface DesignTableResult {
  table: DesignTableModel
  storage: {
    engine: string | null
    charset: string | null
    collation: string | null
    tablespace: string | null
    row_count: number | null
  }
  fingerprint: string
}

export interface DesignRenames {
  columns: Record<string, string>
  indexes: Record<string, string>
}

export interface DesignRequest {
  schema: string
  /** 已有表的名字；新建表时为 null。 */
  name: string | null
  draft: DesignTableModel
  renames: DesignRenames
  rebuild: boolean
  fingerprint: string | null
}

export type TableChange =
  | { kind: 'create_table'; table: DesignTableModel }
  | { kind: 'set_table_comment'; comment: string | null }
  | { kind: 'add_column'; column: DesignColumn; after: string | null }
  | { kind: 'drop_column'; name: string }
  | { kind: 'rename_column'; from: string; to: string; column: DesignColumn }
  | { kind: 'alter_column'; from: DesignColumn; to: DesignColumn }
  | { kind: 'move_column'; column: DesignColumn; after: string | null }
  | { kind: 'drop_primary_key' | 'add_primary_key'; key: DesignKey }
  | { kind: 'drop_unique' | 'add_unique'; unique: DesignKey }
  | { kind: 'drop_check' | 'add_check'; check: DesignCheck }
  | { kind: 'drop_foreign_key' | 'add_foreign_key'; key: DesignForeignKey }
  | { kind: 'drop_index' | 'add_index'; index: DesignIndex }
  | { kind: 'rename_index'; from: string; index: DesignIndex }

export type DesignSection = 'table' | 'columns' | 'primary_key' | 'indexes' | 'foreign_keys' | 'uniques' | 'checks'

export interface DesignIssue {
  section: DesignSection
  row: number | null
  field: string | null
  message: string
}

/** 预览中的一条：SQL，或由程序执行的核对行数。`changes` 是对应的变更序号。 */
export type DesignStep =
  | { kind: 'sql'; sql: string; changes: number[] }
  | { kind: 'count_check'; source: string; target: string; changes: number[] }

export interface DesignPreview {
  errors: DesignIssue[]
  changes: TableChange[]
  statements: DesignStep[]
  rebuild: { reasons: string[]; active: boolean; refused: string | null }
  warnings: string[]
  transactional: boolean
  denied: string | null
  confirm: string[]
  conflict: boolean
}

export type FetchFn = (input: string, init?: RequestInit) => Promise<Response>

export function createApi(token: string, fetchFn: FetchFn = (i, o) => fetch(i, o)) {
  async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
    const headers: Record<string, string> = { Authorization: `Bearer ${token}`, 'Accept-Language': currentLang() }
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

  const p = (id: string) => `/profiles/${encodeURIComponent(id)}`
  const s = (id: string, schema: string) => `${p(id)}/schemas/${encodeURIComponent(schema)}`
  const q = (params: Record<string, string | undefined>) => {
    const entries = Object.entries(params).filter((e): e is [string, string] => e[1] !== undefined && e[1] !== '')
    return entries.length ? `?${new URLSearchParams(entries)}` : ''
  }

  return {
    settings: () => request<Record<string, string>>('GET', '/settings'),
    putSetting: (key: string, value: string | null) =>
      request<null>('PUT', `/settings/${encodeURIComponent(key)}`, { value }),
    drivers: () => request<DriverStatus[]>('GET', '/drivers'),
    setLibDir: (driver: DriverId, path: string | null) =>
      request<DriverStatus>('PUT', `/drivers/${driver}/lib-dir`, { path }),
    secrets: () => request<SecretStatus>('GET', '/secrets'),
    initVault: (master: string) => request<SecretStatus>('POST', '/secrets/init', { master }),
    unlockVault: (master: string) => request<SecretStatus>('POST', '/secrets/unlock', { master }),
    lockVault: () => request<SecretStatus>('POST', '/secrets/lock'),
    profiles: () => request<Profile[]>('GET', '/profiles'),
    profile: (id: string) => request<Profile>('GET', p(id)),
    createProfile: (profile: ProfileInput, password: string | null) =>
      request<Profile>('POST', '/profiles', { profile, password }),
    updateProfile: (id: string, profile: ProfileInput) => request<Profile>('PUT', p(id), profile),
    setPassword: (id: string, password: string | null) => request<null>('PUT', `${p(id)}/password`, { password }),
    deleteProfile: (id: string) => request<null>('DELETE', p(id)),
    testConnection: (profile: ProfileInput, password: string) =>
      request<ServerSummary>('POST', '/profiles/test', { profile, password }),
    schemas: (id: string, filter?: string) => request<SchemaInfo[]>('GET', `${p(id)}/schemas${q({ filter })}`),
    objects: (id: string, schema: string, kind: ObjectKind, filter?: string) =>
      request<ObjectInfo[]>('GET', `${s(id, schema)}/objects${q({ kind, filter })}`),
    table: async (id: string, schema: string, table: string) =>
      (await request<TableInfo[]>('GET', `${s(id, schema)}/tables${q({ table })}`))[0] ?? null,
    sequences: (id: string, schema: string) => request<SequenceInfo[]>('GET', `${s(id, schema)}/sequences`),
    routines: (id: string, schema: string) => request<RoutineInfo[]>('GET', `${s(id, schema)}/routines`),
    synonyms: (id: string, schema: string) => request<SynonymInfo[]>('GET', `${s(id, schema)}/synonyms`),
    ddl: async (id: string, schema: string, kind: DdlKind, name: string) =>
      (await request<{ ddl: string }>('GET', `${s(id, schema)}/ddl${q({ kind, name })}`)).ddl,
    generate: async (id: string, schema: string, g: GenerateParams) =>
      (
        await request<{ sql: string }>(
          'GET',
          `${s(id, schema)}/generate${q({
            object: g.object,
            object_kind: g.objectKind,
            kind: g.kind,
            ref_schema: g.ref?.schema,
            ref_table: g.ref?.table,
          })}`,
        )
      ).sql,
    relations: (id: string, schema: string, table: string) =>
      request<Relation[]>('GET', `${s(id, schema)}/relations${q({ table })}`),
    dataSql: async (id: string, schema: string, d: DataSqlParams) =>
      (await request<{ sql: string }>('POST', `${s(id, schema)}/data-sql`, d)).sql,
    /** 清掉后端的元数据缓存；不给 schema 时清整个连接的。 */
    refresh: (id: string, schema: string | null) => request<null>('POST', `${p(id)}/refresh`, { schema }),
    history: (p: HistoryParams = {}) =>
      request<HistoryItem[]>(
        'GET',
        `/history${q({
          q: p.q,
          profile: p.profile,
          failed: p.failed ? 'true' : undefined,
          before: p.before === undefined ? undefined : String(p.before),
          limit: p.limit === undefined ? undefined : String(p.limit),
        })}`,
      ),
    deleteHistory: (id: number) => request<null>('DELETE', `/history/${id}`),
    clearHistory: () => request<null>('DELETE', '/history'),
    favorites: () => request<Favorite[]>('GET', '/favorites'),
    createFavorite: (f: NewFavorite) => request<Favorite>('POST', '/favorites', f),
    updateFavorite: (id: string, patch: FavoritePatch) =>
      request<Favorite>('PUT', `/favorites/${encodeURIComponent(id)}`, patch),
    deleteFavorite: (id: string) => request<null>('DELETE', `/favorites/${encodeURIComponent(id)}`),
    ops: (id: string) => request<OpsQuery[]>('GET', `${p(id)}/ops`),
    previewChanges: (id: string, set: ChangeSet) =>
      request<{ index: number; sql: string }[]>('POST', `${p(id)}/changes/preview`, set),
    applyChanges: (sessionId: number, set: ChangeSet) =>
      request<ApplyOutcome>('POST', `/sessions/${sessionId}/changes`, set),
    startExport: async (req: ExportRequest) => (await request<{ id: string }>('POST', '/exports', req)).id,
    exportStatus: (id: string) => request<ExportStatus>('GET', `/exports/${encodeURIComponent(id)}`),
    /** 进行中的导出取消；已结束的删除记录。 */
    cancelExport: (id: string) => request<null>('DELETE', `/exports/${encodeURIComponent(id)}`),
    /** 浏览器模式的下载地址：链接不能带请求头，令牌放在查询参数里。 */
    exportFileUrl: (id: string) => `/api/exports/${encodeURIComponent(id)}/file?token=${encodeURIComponent(token)}`,
    /** 上传文件（浏览器模式的导入、执行 SQL 文件），`onProgress` 收到已传的比例（0–1）。用 XHR：fetch 不能报告上传进度。 */
    upload: (file: File, onProgress?: (ratio: number) => void) =>
      new Promise<UploadInfo>((resolve, reject) => {
        const xhr = new XMLHttpRequest()
        xhr.open('POST', `/api/uploads?name=${encodeURIComponent(file.name)}`)
        xhr.setRequestHeader('Authorization', `Bearer ${token}`)
        xhr.setRequestHeader('Accept-Language', currentLang())
        xhr.upload.onprogress = (e) => {
          if (e.lengthComputable) onProgress?.(e.loaded / e.total)
        }
        xhr.onload = () => {
          let data: unknown = null
          try {
            data = xhr.responseText ? JSON.parse(xhr.responseText) : null
          } catch {
            data = null
          }
          if (xhr.status >= 200 && xhr.status < 300) resolve(data as UploadInfo)
          else reject(new ApiError(xhr.status, (data as ErrorBody | null) ?? { kind: 'internal', message: `HTTP ${xhr.status}` }))
        }
        xhr.onerror = () => reject(new ApiError(0, { kind: 'internal', message: t('common.networkError') }))
        xhr.send(file)
      }),
    importPreview: (source: FileSource, file: FileOptions) =>
      request<FilePreview>('POST', '/imports/preview', { source, file }),
    /** 新建表的建表语句预览。 */
    importDdl: async (id: string, schema: string, table: string, create: NewTable) =>
      (await request<{ sql: string }>('POST', `${p(id)}/imports/ddl`, { schema, table, create })).sql,
    startImport: async (req: ImportRequest) => (await request<{ id: string }>('POST', '/imports', req)).id,
    startSqlFile: async (req: SqlFileRequest) => (await request<{ id: string }>('POST', '/sql-files', req)).id,
    task: (id: string) => request<TaskStatus>('GET', `/tasks/${encodeURIComponent(id)}`),
    /** 进行中的任务取消；已结束的删除记录和报告。 */
    cancelTask: (id: string) => request<null>('DELETE', `/tasks/${encodeURIComponent(id)}`),
    confirmTask: (id: string, yes: boolean) => request<null>('POST', `/tasks/${encodeURIComponent(id)}/confirm`, { yes }),
    /** 报告（出错报告或执行日志）的下载地址。 */
    taskReportUrl: (id: string) => `/api/tasks/${encodeURIComponent(id)}/report?token=${encodeURIComponent(token)}`,
    /** 桌面版：把报告复制到"另存为"选的路径。 */
    saveTaskReport: (id: string, path: string) =>
      request<null>('POST', `/tasks/${encodeURIComponent(id)}/report`, { path }),
    /** 前 100 条失败中第 n 条的完整语句。 */
    taskStatement: async (id: string, n: number) =>
      (await request<{ sql: string }>('GET', `/tasks/${encodeURIComponent(id)}/statements/${n}`)).sql,
    designCaps: (id: string) => request<DesignCaps>('GET', `${p(id)}/design/caps`),
    designTable: (id: string, schema: string, name: string) =>
      request<DesignTableResult>('GET', `${p(id)}/design/table${q({ schema, name })}`),
    previewDesign: (id: string, req: DesignRequest) => request<DesignPreview>('POST', `${p(id)}/design/preview`, req),
    applyDesign: async (id: string, req: DesignRequest) =>
      (await request<{ id: string }>('POST', `${p(id)}/design/apply`, req)).id,
    openSession: (profileId: string) => request<SessionInfo>('POST', '/sessions', { profile_id: profileId }),
    session: (id: number) => request<SessionInfo>('GET', `/sessions/${id}`),
    /** 关闭会话；有未提交的修改时按 commit 提交或回滚。 */
    closeSession: (id: number, commit = false) => request<null>('DELETE', `/sessions/${id}?commit=${commit}`),
    setAutocommit: (id: number, on: boolean) => request<SessionInfo>('POST', `/sessions/${id}/autocommit`, { on }),
    commit: (id: number) => request<SessionInfo>('POST', `/sessions/${id}/commit`),
    rollback: (id: number) => request<SessionInfo>('POST', `/sessions/${id}/rollback`),
    reconnect: (id: number) => request<SessionInfo>('POST', `/sessions/${id}/reconnect`),
    setSchema: (id: number, schema: string) => request<SessionInfo>('POST', `/sessions/${id}/schema`, { schema }),
    /** 会话的 WebSocket 地址。浏览器的 WebSocket 不能带请求头，令牌和语言放在查询参数里。 */
    wsUrl: (sessionId: number, loc: { protocol: string; host: string } = window.location) =>
      `${loc.protocol === 'https:' ? 'wss' : 'ws'}://${loc.host}/api/sessions/${sessionId}/ws?token=${encodeURIComponent(token)}&lang=${currentLang()}`,
  }
}

export type Api = ReturnType<typeof createApi>
