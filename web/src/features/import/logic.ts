// 导入向导的纯逻辑（M2b 设计文档第 3 节）：文件格式、列的自动映射、新建表的列和类型、预览单元格的校验。

import type { ColumnInfo, LogicalType, NewTable, TableInfo, WriteMode } from '../../shared/api'
import type { CellValue } from '../../shared/exec'
import { parseCell } from '../data/parse'

/** 按扩展名判断文件格式；不支持的返回 null。 */
export function fileFormat(name: string): 'csv' | 'excel' | null {
  const ext = name.toLowerCase().split('.').pop() ?? ''
  if (['csv', 'tsv', 'txt'].includes(ext)) return 'csv'
  if (['xlsx', 'xls'].includes(ext)) return 'excel'
  return null
}

/** 系统"打开"对话框的文件类型。 */
export const IMPORT_EXTENSIONS = ['csv', 'tsv', 'txt', 'xlsx', 'xls']

/** 路径中的文件名部分（Windows 和 Unix 的分隔符都认）。 */
export function fileNameOf(path: string): string {
  return path.split(/[\\/]/).pop() ?? path
}

const norm = (s: string) => s.trim().toLowerCase()

/** 自动映射：文件的每一列对应同名的表列（去掉首尾空白，不区分大小写），没有的为 null；一个表列只用一次。 */
export function autoMap(fileColumns: string[], tableColumns: string[]): (string | null)[] {
  const used = new Set<string>()
  return fileColumns.map((f) => {
    const hit = tableColumns.find((c) => !used.has(c) && norm(c) === norm(f))
    if (hit) used.add(hit)
    return hit ?? null
  })
}

/** 新建表的默认表名：文件名去掉扩展名；字母、数字、下划线和中文以外的字符换成下划线；以数字开头时加 `t_`。 */
export function tableNameFromFile(name: string): string {
  const stem = name.replace(/\.[^.]*$/, '')
  let s = stem.replace(/[^\p{L}\p{N}_]+/gu, '_').replace(/^_+|_+$/g, '')
  if (!s) s = 'import'
  return /^\d/.test(s) ? `t_${s}` : s
}

/** 按主键更新不可选的原因：表没有主键，或主键列没有全部映射；可选时为 null。 */
export function upsertBlocked(table: TableInfo | null, mapped: string[]): 'no_key' | 'key_unmapped' | null {
  const pk = table?.primary_key?.columns ?? []
  if (!pk.length) return 'no_key'
  return pk.every((k) => mapped.includes(k)) ? null : 'key_unmapped'
}

/** 写入方式是否可用：新建表只能追加。 */
export function modeAllowed(mode: WriteMode, isNew: boolean, blocked: string | null): boolean {
  if (isNew) return mode === 'append'
  return mode !== 'upsert' || blocked === null
}

/** 预览中的一个单元格写入某列时的错误（与表数据编辑的解析规则相同）；没有错误为 null。
 * 只检查文本单元格：Excel 的数字、日期等已有类型，由后端转换。 */
export function cellError(cell: CellValue, col: ColumnInfo): string | null {
  if (cell.t !== 'string') return null
  const r = parseCell(String(cell.v), col.data_type.logical as LogicalType)
  return 'error' in r ? r.error : null
}

/** 新建表时可选的类型。 */
export const TYPE_CHOICES = ['int', 'bigint', 'decimal', 'float', 'bool', 'var_char', 'text', 'date', 'time', 'timestamp'] as const
export type TypeChoice = (typeof TYPE_CHOICES)[number]

/** 新建表的一列（界面编辑用）：长度、精度只对需要的类型有意义。 */
export interface ColumnDraft {
  name: string
  type: TypeChoice
  len: number
  precision: number
  scale: number
  nullable: boolean
  key: boolean
}

/** 推断出的逻辑类型 → 界面的选项。 */
export function draftOf(name: string, logical: LogicalType): ColumnDraft {
  const base = { name, len: 50, precision: 10, scale: 2, nullable: true, key: false }
  switch (logical.kind) {
    case 'int':
    case 'u_int':
      return { ...base, type: logical.bits > 32 ? 'bigint' : 'int' }
    case 'decimal':
      return { ...base, type: 'decimal', precision: logical.precision ?? 10, scale: logical.scale ?? 0 }
    case 'float':
      return { ...base, type: 'float' }
    case 'bool':
    case 'date':
    case 'time':
    case 'timestamp':
    case 'text':
      return { ...base, type: logical.kind }
    case 'var_char':
    case 'char':
      return { ...base, type: 'var_char', len: logical.len ?? 50 }
    default:
      return { ...base, type: 'var_char' }
  }
}

/** 界面的选项 → 逻辑类型。 */
export function logicalOf(c: ColumnDraft): LogicalType {
  switch (c.type) {
    case 'int':
      return { kind: 'int', bits: 32 }
    case 'bigint':
      return { kind: 'int', bits: 64 }
    case 'decimal':
      return { kind: 'decimal', precision: c.precision, scale: c.scale }
    case 'float':
      return { kind: 'float', bits: 64 }
    case 'var_char':
      return { kind: 'var_char', len: c.len }
    case 'time':
      return { kind: 'time', precision: null }
    case 'timestamp':
      return { kind: 'timestamp', tz: false, precision: null }
    default:
      return { kind: c.type }
  }
}

/** 新建表的定义。 */
export function newTable(columns: ColumnDraft[]): NewTable {
  return {
    columns: columns.map((c) => ({ name: c.name.trim(), logical: logicalOf(c), nullable: c.nullable && !c.key })),
    primary_key: columns.filter((c) => c.key).map((c) => c.name.trim()),
  }
}

/** 新建表的列定义有没有问题：空名、重名（不区分大小写）。 */
export function draftProblem(columns: ColumnDraft[]): 'empty' | 'duplicate' | null {
  const names = columns.map((c) => norm(c.name))
  if (names.some((n) => !n)) return 'empty'
  return new Set(names).size === names.length ? null : 'duplicate'
}

/** 字节数的显示：KB、MB。 */
export function sizeText(bytes: number): string {
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}
