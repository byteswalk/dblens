import { describe, expect, it } from 'vitest'

import type { ResultSet } from '../../shared/exec'
import { sortedRows } from './ResultGrid'

const set = (values: (string | null)[]) =>
  ({
    columns: [{ name: 'v' }],
    rows: values.map((v) => [v === null ? { t: 'null' } : { t: 'string', v }]),
  }) as unknown as ResultSet

describe('结果表排序', () => {
  it('不排序时保持原顺序', () => {
    expect(sortedRows(set(['b', 'a']), null)).toEqual([0, 1])
  })

  it('文字升序、降序，NULL 总在最后', () => {
    const r = set(['b', null, 'a'])
    expect(sortedRows(r, { column: 'v', desc: false })).toEqual([2, 0, 1])
    expect(sortedRows(r, { column: 'v', desc: true })).toEqual([0, 2, 1])
  })

  it('带数字的文字按数值大小', () => {
    expect(sortedRows(set(['a10', 'a2']), { column: 'v', desc: false })).toEqual([1, 0])
  })
})
