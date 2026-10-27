// 数据标签的编辑状态（M2a 设计文档 6.1、6.2 节）：修改、新增、删除都先暂存在这里，提交时转成改动集。纯函数，单独测试。
// 表格的行：先是已加载的行（0 … loaded-1），后面接新增的行（loaded + i）。

import type { RowChange } from '../../shared/api'
import type { CellValue } from '../../shared/exec'

/** 暂存的单元格：解析成功的值，或解析失败的原文和原因。 */
export type Pending = { value: CellValue } | { error: string; text: string }

export interface EditState {
  /** 已加载行的修改：行号 → 列号 → 新值。 */
  updates: Record<number, Record<number, Pending>>
  /** 新增的行，按列号存；没填的列不写（交给数据库的默认值）。 */
  inserts: Record<number, Pending>[]
  /** 删除的已加载行号，升序。 */
  deletes: number[]
}

export const emptyEdit: EditState = { updates: {}, inserts: [], deletes: [] }

export function sameCell(a: CellValue, b: CellValue): boolean {
  return a.t === b.t && JSON.stringify(a.v) === JSON.stringify(b.v)
}

/** 设置一个单元格。已加载的行改回原值时取消这个改动；删除了的行不能改。 */
export function setCell(s: EditState, loaded: number, row: number, col: number, cell: Pending, original?: CellValue): EditState {
  if (row >= loaded) {
    const i = row - loaded
    if (i >= s.inserts.length) return s
    const inserts = s.inserts.slice()
    inserts[i] = { ...inserts[i], [col]: cell }
    return { ...s, inserts }
  }
  if (s.deletes.includes(row)) return s
  const cols = { ...s.updates[row] }
  if (original && 'value' in cell && sameCell(cell.value, original)) delete cols[col]
  else cols[col] = cell
  const updates = { ...s.updates }
  if (Object.keys(cols).length) updates[row] = cols
  else delete updates[row]
  return { ...s, updates }
}

/** 末尾新增 `count` 行；给出 `template` 时每行以它为底稿（复制行）。 */
export function addRows(s: EditState, count = 1, template: Record<number, Pending> = {}): EditState {
  return { ...s, inserts: [...s.inserts, ...Array.from({ length: count }, () => ({ ...template }))] }
}

/** 删除行：已加载的行记为删除（丢掉它的修改），新增的行直接移除。 */
export function deleteRows(s: EditState, loaded: number, rows: number[]): EditState {
  const loadedRows = rows.filter((r) => r < loaded)
  const removed = new Set(rows.filter((r) => r >= loaded).map((r) => r - loaded))
  const updates = { ...s.updates }
  for (const r of loadedRows) delete updates[r]
  return {
    updates,
    inserts: s.inserts.filter((_, i) => !removed.has(i)),
    deletes: [...new Set([...s.deletes, ...loadedRows])].sort((a, b) => a - b),
  }
}

/** 撤销一行的全部改动：修改、删除取消，新增的行移除。 */
export function undoRow(s: EditState, loaded: number, row: number): EditState {
  if (row >= loaded) return { ...s, inserts: s.inserts.filter((_, i) => i !== row - loaded) }
  const updates = { ...s.updates }
  delete updates[row]
  return { ...s, updates, deletes: s.deletes.filter((r) => r !== row) }
}

/** 撤销一个单元格的修改（新增行的单元格清空）。 */
export function undoCell(s: EditState, loaded: number, row: number, col: number): EditState {
  if (row >= loaded) {
    const i = row - loaded
    if (i >= s.inserts.length) return s
    const inserts = s.inserts.slice()
    const { [col]: _, ...rest } = inserts[i]
    inserts[i] = rest
    return { ...s, inserts }
  }
  const cols = { ...s.updates[row] }
  delete cols[col]
  const updates = { ...s.updates }
  if (Object.keys(cols).length) updates[row] = cols
  else delete updates[row]
  return { ...s, updates }
}

/** 改动数：修改的行、新增的行、删除的行各算一个。 */
export function changeCount(s: EditState): number {
  return Object.keys(s.updates).length + s.inserts.length + s.deletes.length
}

/** 有解析失败的单元格时不能提交。 */
export function hasErrors(s: EditState): boolean {
  const bad = (cells: Record<number, Pending>) => Object.values(cells).some((c) => 'error' in c)
  return Object.values(s.updates).some(bad) || s.inserts.some(bad)
}

/** 粘贴时每列的信息：能否编辑、怎样解析。 */
export interface PasteColumn {
  editable: boolean
  parse(text: string): Pending
}

/**
 * 从 `anchor` 开始向右下覆盖：覆盖已加载的行算修改（删除了的行跳过），超出已加载和已新增的行时新增行。
 * 超出表格列数的列忽略，返回忽略的列数；不能编辑的列跳过。
 */
export function paste(
  s: EditState,
  loaded: number,
  rows: CellValue[][],
  anchor: { row: number; col: number },
  matrix: string[][],
  columns: PasteColumn[],
): { state: EditState; ignoredColumns: number } {
  let state = s
  let ignored = 0
  matrix.forEach((cells, r) => {
    const row = anchor.row + r
    // 需要时补新增行
    const need = row - loaded - state.inserts.length + 1
    if (row >= loaded && need > 0) state = addRows(state, need)
    cells.forEach((text, c) => {
      const col = anchor.col + c
      if (col >= columns.length) {
        ignored = Math.max(ignored, cells.length - c)
        return
      }
      if (!columns[col].editable) return
      state = setCell(state, loaded, row, col, columns[col].parse(text), row < loaded ? rows[row][col] : undefined)
    })
  })
  return { state, ignoredColumns: ignored }
}

/**
 * 转成提交给后端的改动集。`keyCols` 是行标识列的列号。
 * `rowOf[i]` 是第 i 个改动在表格中的行号，执行失败时据此标出那一行。调用前先用 `hasErrors` 检查。
 */
export function buildChanges(
  s: EditState,
  loaded: number,
  rows: CellValue[][],
  columns: { name: string }[],
  keyCols: number[],
): { changes: RowChange[]; rowOf: number[] } {
  const changes: RowChange[] = []
  const rowOf: number[] = []
  const key = (row: number) => Object.fromEntries(keyCols.map((c) => [columns[c].name, rows[row][c]]))
  const value = (p: Pending): CellValue => ('value' in p ? p.value : { t: 'null' })
  for (const row of s.deletes) {
    changes.push({ kind: 'delete', key: key(row) })
    rowOf.push(row)
  }
  for (const [r, cells] of Object.entries(s.updates)) {
    const row = Number(r)
    const cols = Object.keys(cells).map(Number)
    changes.push({
      kind: 'update',
      key: key(row),
      original: Object.fromEntries(cols.map((c) => [columns[c].name, rows[row][c]])),
      values: Object.fromEntries(cols.map((c) => [columns[c].name, value(cells[c])])),
    })
    rowOf.push(row)
  }
  s.inserts.forEach((cells, i) => {
    changes.push({
      kind: 'insert',
      values: Object.fromEntries(Object.entries(cells).map(([c, p]) => [columns[Number(c)].name, value(p)])),
    })
    rowOf.push(loaded + i)
  })
  return { changes, rowOf }
}
