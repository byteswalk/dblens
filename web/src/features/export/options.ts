// 导出对话框的纯逻辑（M1c3 设计文档 3.4、4.2 节）：默认选项、默认文件名、另存为对话框的文件类型。

import type { ExportFormat, ExportOptions } from '../../shared/api'

export const FORMATS: ExportFormat[] = ['csv', 'xlsx', 'insert', 'json']

export const EXTENSION: Record<ExportFormat, string> = { csv: 'csv', xlsx: 'xlsx', insert: 'sql', json: 'json' }

/** 默认选项；`table` 为 INSERT 的目标表（数据标签为对象的限定名，查询结果为占位的 `table_name`）。 */
export function defaultOptions(table = 'table_name'): ExportOptions {
  return { delimiter: ',', header: true, bom: true, null_text: '', table, rows_per_insert: 1 }
}

/** 文件名里不能出现的字符换成下划线。 */
function safe(name: string): string {
  const s = name.replace(/[\\/:*?"<>|\s]+/g, '_').replace(/^_+|_+$/g, '')
  return s || 'export'
}

/** 默认文件名：`对象名_年月日_时分秒.扩展名`。 */
export function defaultFileName(base: string, format: ExportFormat, now = new Date()): string {
  const p = (n: number) => String(n).padStart(2, '0')
  const stamp = `${now.getFullYear()}${p(now.getMonth() + 1)}${p(now.getDate())}_${p(now.getHours())}${p(now.getMinutes())}${p(now.getSeconds())}`
  return `${safe(base)}_${stamp}.${EXTENSION[format]}`
}

/** 另存为对话框的文件类型。 */
export function fileFilter(format: ExportFormat, label: string): { name: string; extensions: string[] } {
  return { name: label, extensions: [EXTENSION[format]] }
}

/** 路径中的文件名部分（Windows 和 Unix 的分隔符都认）。 */
export function baseName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path
}
