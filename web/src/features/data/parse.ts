// 表数据编辑的文本解析（M2a 设计文档 6.3 节）：编辑框输入和粘贴的文本按列的逻辑类型转成单元格值。
// 值的格式与结果集相同（{t, v}），提交时原样交给后端绑定参数。

import type { CellValue } from '../../shared/exec'
import { t } from '../../shared/i18n'

/** 列的逻辑类型（dblens_core::LogicalType 的 JSON，按 kind 区分）。 */
export interface Logical {
  kind: string
  bits?: number
  tz?: boolean
}

export type Parsed = { value: CellValue } | { error: string }

const TEXT_KINDS = new Set(['char', 'var_char', 'text'])

/** 不能在表格里编辑的类型：二进制和大字段。 */
export function editableKind(logical: Logical): boolean {
  return !['binary', 'var_binary', 'blob'].includes(logical.kind)
}

const pad = (n: number | string, w = 2) => String(n).padStart(w, '0')

function date(text: string): string | null {
  const m = /^(\d{4})[-/](\d{1,2})[-/](\d{1,2})$/.exec(text)
  if (!m) return null
  const [y, mo, d] = [Number(m[1]), Number(m[2]), Number(m[3])]
  const dt = new Date(Date.UTC(y, mo - 1, d))
  if (dt.getUTCFullYear() !== y || dt.getUTCMonth() !== mo - 1 || dt.getUTCDate() !== d) return null
  return `${m[1]}-${pad(mo)}-${pad(d)}`
}

function time(text: string): string | null {
  const m = /^(\d{1,2}):(\d{2})(?::(\d{2})(\.\d{1,9})?)?$/.exec(text)
  if (!m) return null
  const [h, mi, s] = [Number(m[1]), Number(m[2]), Number(m[3] ?? 0)]
  if (h > 23 || mi > 59 || s > 59) return null
  return `${pad(h)}:${pad(mi)}:${pad(s)}${m[4] ?? ''}`
}

/** 本地时区的偏移，如 `+08:00`。 */
function localOffset(): string {
  const off = -new Date().getTimezoneOffset()
  const sign = off >= 0 ? '+' : '-'
  return `${sign}${pad(Math.floor(Math.abs(off) / 60))}:${pad(Math.abs(off) % 60)}`
}

/** 文本按逻辑类型解析。空文本：文本类为空字符串，其他类型为 NULL。 */
export function parseCell(text: string, logical: Logical): Parsed {
  const s = text.trim()
  const fail = (kind: string) => ({ error: t('editor.data.edit.bad', { kind }) })
  if (text === '') return { value: TEXT_KINDS.has(logical.kind) ? { t: 'string', v: '' } : { t: 'null' } }
  switch (logical.kind) {
    case 'bool': {
      const v = s.toLowerCase()
      if (v === 'true' || v === '1') return { value: { t: 'bool', v: true } }
      if (v === 'false' || v === '0') return { value: { t: 'bool', v: false } }
      return fail(t('editor.data.edit.kind.bool'))
    }
    case 'int':
    case 'u_int': {
      if (!/^[+-]?\d+$/.test(s)) return fail(t('editor.data.edit.kind.int'))
      const n = BigInt(s)
      const bits = BigInt(logical.bits ?? 64)
      const [min, max] =
        logical.kind === 'int' ? [-(1n << (bits - 1n)), (1n << (bits - 1n)) - 1n] : [0n, (1n << bits) - 1n]
      if (n < min || n > max) return { error: t('editor.data.edit.outOfRange') }
      // 超出 JS 安全整数范围的按精确小数传，避免丢精度
      return Number.isSafeInteger(Number(n))
        ? { value: { t: 'int', v: Number(n) } }
        : { value: { t: 'decimal', v: n.toString() } }
    }
    case 'float': {
      const n = Number(s)
      return s !== '' && Number.isFinite(n) ? { value: { t: 'float', v: n } } : fail(t('editor.data.edit.kind.number'))
    }
    case 'decimal':
      return /^[+-]?(\d+(\.\d*)?|\.\d+)([eE][+-]?\d+)?$/.test(s)
        ? { value: { t: 'decimal', v: s } }
        : fail(t('editor.data.edit.kind.number'))
    case 'date': {
      const d = date(s)
      return d ? { value: { t: 'date', v: d } } : fail(t('editor.data.edit.kind.date'))
    }
    case 'time': {
      const v = time(s)
      return v ? { value: { t: 'time', v } } : fail(t('editor.data.edit.kind.time'))
    }
    case 'timestamp': {
      const m = /^(\S+)[ T](\d{1,2}:\d{2}(?::\d{2}(?:\.\d{1,9})?)?)\s*(Z|[+-]\d{2}:?\d{2})?$/.exec(s)
      const d = m && date(m[1])
      const tm = m && time(m[2])
      if (!m || !d || !tm) return fail(t('editor.data.edit.kind.timestamp'))
      if (!logical.tz) return { value: { t: 'date_time', v: `${d}T${tm}` } }
      // 带时区的类型：没写时区时按本地时区
      let off = m[3] ?? localOffset()
      if (off === 'Z') off = '+00:00'
      else if (!off.includes(':')) off = `${off.slice(0, 3)}:${off.slice(3)}`
      return { value: { t: 'date_time_tz', v: `${d}T${tm}${off}` } }
    }
    case 'uuid':
      return /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(s)
        ? { value: { t: 'uuid', v: s.toLowerCase() } }
        : fail('UUID')
    case 'json':
      try {
        return { value: { t: 'json', v: JSON.parse(text) } }
      } catch {
        return fail('JSON')
      }
    case 'interval':
      return { value: { t: 'interval', v: s } }
    default:
      return { value: { t: 'string', v: text } }
  }
}

/**
 * 解析从 Excel 等表格复制的制表符分隔文本：行之间是换行，单元格之间是制表符；
 * 含制表符、换行或引号的单元格用双引号包住，里面的引号写成两个。末尾的空行忽略。
 */
export function parseTsv(text: string): string[][] {
  const rows: string[][] = []
  let row: string[] = []
  let cell = ''
  let i = 0
  let quoted = false
  while (i < text.length) {
    const c = text[i]
    if (quoted) {
      if (c === '"' && text[i + 1] === '"') {
        cell += '"'
        i += 2
        continue
      }
      if (c === '"') quoted = false
      else cell += c
      i += 1
      continue
    }
    if (c === '"' && cell === '') quoted = true
    else if (c === '\t') {
      row.push(cell)
      cell = ''
    } else if (c === '\n' || c === '\r') {
      row.push(cell)
      rows.push(row)
      row = []
      cell = ''
      if (c === '\r' && text[i + 1] === '\n') i += 1
    } else cell += c
    i += 1
  }
  if (cell !== '' || row.length) {
    row.push(cell)
    rows.push(row)
  }
  return rows
}
