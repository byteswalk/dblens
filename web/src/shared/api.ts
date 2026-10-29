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
  /** 新建连接时选的产品（如 kingbase）；没有时按驱动显示。连上后以识别出的变体为准。 */
  product?: string | null
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
  /** 兼容模式：人大金仓、openGauss 可能是 oracle。 */
  compat: Compat
  /** 探测到的兼容模式名（人大金仓的 database_mode、openGauss 当前库的 DBCOMPATIBILITY）。 */
  mode: string | null
  /** 模式名不在已适配的范围内（如 openGauss 的 B 模式）。 */
  unadapted_mode: boolean
}

export type Compat = 'native' | 'oracle'

export interface SecretStatus {
  backend: 'keyring' | 'vault'
  vault_initialized: boolean
  unlocked: boolean
}

export interface SessionInfo {
  id: number
  profile_id: string
  variant: string
  compat: Compat
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
  kind: 'import' | 'sql_file' | 'design' | 'reverse' | 'sync' | 'document'
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
  /** 反向生成：生成并打开的模型的编号。 */
  model: string | null
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
    /** 主键是否聚簇（只有 TiDB 报告）。 */
    clustered?: boolean | null
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

/** 模型（M4a）：绑定的数据库类型。`version` 是编码后的版本，不写时按最新版本。 */
export interface ModelDatabase {
  variant: string
  compat?: Compat
  version?: number | null
}

export interface ModelInfo {
  format: number
  name: string
  description?: string | null
  database: ModelDatabase
  schemas: string[]
  renames?: Record<string, { tables?: Record<string, string>; columns?: Record<string, Record<string, string>> }>
}

export interface ModelSummary {
  id: string
  path: string
  info: ModelInfo
  read_only: boolean
}

export interface ModelFileError {
  path: string
  message: string
}

export interface ModelTableEntry {
  schema: string
  name: string
  comment?: string | null
  error?: ModelFileError | null
}

export interface ModelDesignRequest {
  schema: string
  name: string | null
  draft: DesignTableModel
  renames: DesignRenames
  fingerprint: string | null
}

export interface ModelDesignPreview {
  errors: DesignIssue[]
  warnings: string[]
  changes: TableChange[]
  conflict: boolean
}

export interface ReverseRequest {
  profile_id: string
  schemas: string[]
  path: string
  name: string
}

/** 比对的一边：模型的一个 schema，或连接的一个 schema。 */
export type CompareSideRef =
  | { kind: 'model'; model: string; schema: string }
  | { kind: 'database'; profile: string; schema: string }

export interface CompareOptions {
  ignore_comments: boolean
  ignore_column_order: boolean
  ignore_generated_names: boolean
  case_insensitive: boolean
}

/** 采用的改名：源的名字 → 目标的名字；列按源的表名分组。 */
export interface CompareRenames {
  tables: Record<string, string>
  columns: Record<string, Record<string, string>>
}

export interface CompareRequest {
  source: CompareSideRef
  target: CompareSideRef
  options: CompareOptions
  renames: CompareRenames
}

export type SchemaChange =
  | { kind: 'drop_table'; name: string }
  | { kind: 'rename_table'; from: string; to: string }
  | { kind: 'table'; change: TableChange }

export type TableStatus = 'added' | 'removed' | 'renamed' | 'changed' | 'same'

export interface TableDiff {
  status: TableStatus
  source: string | null
  target: string | null
  items: { id: number; change: SchemaChange }[]
  appended: boolean
}

/** 推测的改名：`table` 为 null 时是表的改名，否则是该表（源的名字）中列的改名。 */
export interface RenameSuggestion {
  table: string | null
  source: string
  target: string
}

export interface CompareResult {
  diff: { tables: TableDiff[]; suggestions: RenameSuggestion[] }
  renames: CompareRenames
  source_errors: ModelFileError[]
  target_errors: ModelFileError[]
  source_tables: DesignTableModel[]
  target_tables: DesignTableModel[]
  fingerprint: string
}

export interface SyncRequest {
  compare: CompareRequest
  selected: number[]
  fingerprint: string | null
}

export type SyncStep =
  | { kind: 'sql'; sql: string; items: number[] }
  | {
      kind: 'rebuild'
      table: string
      rebuild: { steps: DesignStep[]; drop_at: number; cleanup: string; warnings: string[] }
      items: number[]
    }

export interface SyncPreview {
  script: { steps: SyncStep[]; refused: [string, string][]; warnings: string[]; transactional: boolean } | null
  text: string
  writes: { name: string; from?: string | null; delete: boolean }[]
  denied: string | null
  confirm: string[]
  fingerprint: string
}

/** 逻辑外键（M4b）：只在模型中记录，不参与比对和同步。 */
export interface LogicalKey {
  name?: string | null
  columns: string[]
  ref_schema?: string | null
  ref_table: string
  ref_columns: string[]
  comment?: string | null
}

export type ErDisplay = 'columns' | 'keys' | 'names'

export interface ErPosition {
  x: number
  y: number
}

/** 图文件：`tables` 为 'all'（默认图）或表名列表。 */
export interface ErDiagram {
  name: string
  tables: 'all' | string[]
  display: ErDisplay
  positions?: Record<string, ErPosition>
  [extra: string]: unknown
}

export interface ErTable {
  table: DesignTableModel
  logical_keys: LogicalKey[]
  error?: ModelFileError | null
}

export type ErWarning =
  | { kind: 'broken_logical_key'; table: string; index: number; name: string | null }
  | { kind: 'missing_table'; name: string }

export interface DiagramView {
  diagram: ErDiagram
  tables: ErTable[]
  fingerprint: string | null
  warnings: ErWarning[]
}

/** 默认图的名字。 */
export const DEFAULT_DIAGRAM = '_all'

/** 文档导出（M4c）：数据字典、设计文档。 */
export type DocKind = 'dictionary' | 'design'
export type DocFormat = 'docx' | 'html' | 'markdown' | 'xlsx'
export type DocSource =
  | { kind: 'model'; model: string; schemas: string[] }
  | { kind: 'database'; profile: string; schema: string }

export interface DocInfo {
  title: string
  version?: string | null
  author?: string | null
  description?: string | null
}

/** 前端画好、已上传的一张 ER 图。 */
export interface DocDiagramImage {
  upload: string
  schema: string
  name: string
  title: string
  description?: string | null
  names_only: boolean
}

export interface DocumentRequest {
  source: DocSource
  tables?: string[] | null
  kind: DocKind
  format: DocFormat
  info: DocInfo
  template?: string | null
  reference?: string | null
  view_definitions: boolean
  diagrams: DocDiagramImage[]
  output?: string | null
}

export interface DocumentProblem {
  field: 'template' | 'reference' | 'output' | 'format'
  error: ErrorBody
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
  const m = (id: string) => `/models/${encodeURIComponent(id)}`
  const md = (id: string, schema: string, name: string) =>
    `${m(id)}/diagrams/${encodeURIComponent(schema)}/${encodeURIComponent(name)}`
  const mt = (id: string, schema: string, name: string) =>
    `${m(id)}/tables/${encodeURIComponent(schema)}/${encodeURIComponent(name)}`
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
    models: () => request<{ open: ModelSummary[]; recent: string[] }>('GET', '/models'),
    openModel: (path: string) => request<ModelSummary>('POST', '/models/open', { path }),
    createModel: (path: string, name: string, database: ModelDatabase) =>
      request<ModelSummary>('POST', '/models', { path, name, database }),
    startReverse: async (req: ReverseRequest) => (await request<{ id: string }>('POST', '/models/reverse', req)).id,
    /** 重新加载并返回模型信息。 */
    model: (id: string) => request<ModelSummary>('GET', m(id)),
    closeModel: (id: string) => request<null>('DELETE', m(id)),
    modelCaps: (id: string) => request<DesignCaps>('GET', `${m(id)}/caps`),
    modelTables: (id: string) => request<ModelTableEntry[]>('GET', `${m(id)}/tables`),
    modelTable: (id: string, schema: string, name: string) =>
      request<{ table: DesignTableModel; fingerprint: string }>('GET', mt(id, schema, name)),
    previewModelTable: (id: string, req: ModelDesignRequest) =>
      request<ModelDesignPreview>('POST', `${m(id)}/tables/preview`, req),
    /** 保存设计器中的表，返回新的指纹。 */
    saveModelTable: async (id: string, req: ModelDesignRequest) =>
      (await request<{ fingerprint: string }>('POST', `${m(id)}/tables`, req)).fingerprint,
    deleteModelTable: (id: string, schema: string, name: string) => request<null>('DELETE', mt(id, schema, name)),
    renameModelTable: (id: string, schema: string, name: string, to: string) =>
      request<null>('POST', `${mt(id, schema, name)}/rename`, { to }),
    diagrams: (id: string, schema: string) =>
      request<{ schema: string; name: string; tables: 'all' | string[] }[]>('GET', `${m(id)}/diagrams${q({ schema })}`),
    createDiagram: async (id: string, schema: string, name: string, tables: string[]) =>
      (await request<{ fingerprint: string }>('POST', `${m(id)}/diagrams`, { schema, name, tables })).fingerprint,
    diagram: (id: string, schema: string, name: string) => request<DiagramView>('GET', md(id, schema, name)),
    /** 保存图，返回新的指纹。 */
    saveDiagram: async (id: string, schema: string, name: string, diagram: ErDiagram, fingerprint: string | null) =>
      (await request<{ fingerprint: string }>('PUT', md(id, schema, name), { diagram, fingerprint })).fingerprint,
    deleteDiagram: (id: string, schema: string, name: string) => request<null>('DELETE', md(id, schema, name)),
    renameDiagram: (id: string, schema: string, name: string, to: string) =>
      request<null>('POST', `${md(id, schema, name)}/rename`, { to }),
    addLogicalKey: (id: string, schema: string, table: string, key: LogicalKey) =>
      request<{ fingerprint: string }>('POST', `${mt(id, schema, table)}/logical-keys`, { key }),
    deleteLogicalKey: (id: string, schema: string, table: string, index: number) =>
      request<{ fingerprint: string }>('DELETE', `${mt(id, schema, table)}/logical-keys/${index}`),
    /** 桌面版：把导出的图片（base64）写到"另存为"选的 .svg、.png 文件。 */
    saveImage: (path: string, data: string) => request<null>('POST', '/files/image', { path, data }),
    startDocument: async (req: DocumentRequest) => (await request<{ id: string }>('POST', '/documents', req)).id,
    /** 开始前检查：模板语法、样式来源、输出位置。 */
    checkDocument: async (req: DocumentRequest) =>
      (await request<{ problems: DocumentProblem[] }>('POST', '/documents/check', req)).problems,
    /** 把内置模板写到指定路径（桌面版）。 */
    exportDocTemplate: (format: 'html' | 'markdown', path: string) =>
      request<null>('POST', '/documents/templates/export', { format, path }),
    /** 整个 schema 的表明细（库的 ER 图）。 */
    schemaTables: (id: string, schema: string) => request<DesignTableModel[]>('GET', `${s(id, schema)}/tables`),
    compare: (req: CompareRequest) => request<CompareResult>('POST', '/compare', req),
    syncPreview: (req: SyncRequest) => request<SyncPreview>('POST', '/compare/preview', req),
    /** 同步：目标是库时返回任务 id；目标是模型时已经写完，返回 null。 */
    sync: async (req: SyncRequest) => (await request<{ task: string | null }>('POST', '/compare/sync', req)).task,
    /** 桌面版：把同步脚本写到"另存为"选的 .sql 文件。 */
    saveScript: (path: string, text: string) => request<null>('POST', '/compare/script', { path, text }),
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
