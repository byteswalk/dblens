// 数据标签中哪些列能编辑、用哪些列标识一行（M2a 设计文档第 3 节）。按表的结构模型判断，纯函数，单独测试。

import type { TableInfo } from '../../shared/api'
import { type Logical, editableKind } from './parse'

export interface EditColumn {
  /** 能否编辑：表中有这一列、不是生成列、不是二进制或大字段类型。 */
  editable: boolean
  logical: Logical
}

export interface EditInfo {
  /** 结果集的每一列，顺序同结果集。 */
  columns: EditColumn[]
  /** 行标识列在结果集中的列号；为 null 时整张表只读。 */
  key: number[] | null
}

/**
 * 行标识：主键；没有主键时取第一个所有列都 NOT NULL 的唯一键。标识列必须都在结果集中。
 * 结果集的列按名称对应表的列（数据标签执行的是 `SELECT *`）。
 */
export function editInfo(resultColumns: string[], table: TableInfo): EditInfo {
  const byName = new Map(table.columns.map((c) => [c.name, c]))
  const columns = resultColumns.map((name) => {
    const c = byName.get(name)
    const logical = (c?.data_type.logical ?? { kind: 'other' }) as Logical
    return { editable: !!c && !c.generated && editableKind(logical), logical }
  })
  const find = (names: string[]) => {
    const idx = names.map((n) => resultColumns.indexOf(n))
    return names.length && idx.every((i) => i >= 0) ? idx : null
  }
  let key = table.primary_key ? find(table.primary_key.columns) : null
  if (!key) {
    for (const u of table.uniques ?? []) {
      if (u.columns.every((n) => byName.get(n)?.nullable === false)) {
        key = find(u.columns)
        if (key) break
      }
    }
  }
  return { columns, key }
}
