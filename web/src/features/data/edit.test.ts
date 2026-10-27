import { describe, expect, it } from 'vitest'

import type { CellValue } from '../../shared/exec'
import {
  type PasteColumn,
  addRows,
  buildChanges,
  changeCount,
  deleteRows,
  emptyEdit,
  hasErrors,
  paste,
  setCell,
  undoCell,
  undoRow,
} from './edit'

const str = (v: string): CellValue => ({ t: 'string', v })
const int = (v: number): CellValue => ({ t: 'int', v })
// 两行已加载的数据：id、name
const rows: CellValue[][] = [
  [int(1), str('甲')],
  [int(2), str('乙')],
]
const columns = [{ name: 'id' }, { name: 'name' }]

describe('编辑状态', () => {
  it('修改：改回原值时取消；删除了的行不能改', () => {
    let s = setCell(emptyEdit, 2, 0, 1, { value: str('丙') }, rows[0][1])
    expect(s.updates).toEqual({ 0: { 1: { value: str('丙') } } })
    s = setCell(s, 2, 0, 1, { value: str('甲') }, rows[0][1])
    expect(s.updates).toEqual({})
    s = deleteRows(s, 2, [1])
    expect(setCell(s, 2, 1, 1, { value: str('x') }, rows[1][1])).toBe(s)
  })

  it('新增、复制、删除新增行、撤销', () => {
    let s = addRows(emptyEdit)
    s = setCell(s, 2, 2, 1, { value: str('新') })
    s = addRows(s, 1, s.inserts[0])
    expect(s.inserts).toEqual([{ 1: { value: str('新') } }, { 1: { value: str('新') } }])
    s = deleteRows(s, 2, [3, 0])
    expect(s.inserts.length).toBe(1)
    expect(s.deletes).toEqual([0])
    expect(changeCount(s)).toBe(2)
    s = undoRow(s, 2, 0)
    s = undoCell(s, 2, 2, 1)
    expect(s).toEqual({ updates: {}, inserts: [{}], deletes: [] })
  })

  it('解析失败的单元格阻止提交', () => {
    const s = setCell(emptyEdit, 2, 0, 0, { error: '不是整数', text: 'x' }, rows[0][0])
    expect(hasErrors(s)).toBe(true)
    expect(hasErrors(undoCell(s, 2, 0, 0))).toBe(false)
  })
})

describe('粘贴', () => {
  const cols: PasteColumn[] = [
    { editable: false, parse: (t) => ({ value: int(Number(t)) }) },
    { editable: true, parse: (t) => ({ value: str(t) }) },
  ]

  it('覆盖已加载的行，超出部分新增行，多出的列忽略，不能编辑的列跳过', () => {
    const { state, ignoredColumns } = paste(emptyEdit, 2, rows, { row: 1, col: 0 }, [
      ['9', '乙改', '多余', '多余'],
      ['9', '新行'],
    ], cols)
    expect(ignoredColumns).toBe(2)
    expect(state.updates).toEqual({ 1: { 1: { value: str('乙改') } } })
    expect(state.inserts).toEqual([{ 1: { value: str('新行') } }])
  })
})

describe('转成改动集', () => {
  it('先删除，再修改，最后新增；修改带行标识和改动列的原值', () => {
    let s = setCell(emptyEdit, 2, 0, 1, { value: str('丙') }, rows[0][1])
    s = deleteRows(s, 2, [1])
    s = addRows(s)
    s = setCell(s, 2, 2, 1, { value: str('新') })
    const { changes, rowOf } = buildChanges(s, 2, rows, columns, [0])
    expect(changes).toEqual([
      { kind: 'delete', key: { id: int(2) } },
      { kind: 'update', key: { id: int(1) }, original: { name: str('甲') }, values: { name: str('丙') } },
      { kind: 'insert', values: { name: str('新') } },
    ])
    expect(rowOf).toEqual([1, 0, 2])
  })
})
