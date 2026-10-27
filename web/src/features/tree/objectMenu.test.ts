import { describe, expect, it } from 'vitest'

import type { Relation } from '../../shared/api'
import { ddlKind, generateChoice, generateMenu, targetKind } from './objectMenu'

const rel = (direction: 'out' | 'in', table: string, cols: string[]): Relation => ({
  direction,
  name: null,
  schema: 's',
  table,
  local_columns: cols,
  other_columns: ['id'],
})

describe('对象的右键菜单', () => {
  it('种类', () => {
    expect(targetKind('materialized_views')).toBe('materialized_view')
    expect(targetKind('sequences')).toBeNull()
    expect(ddlKind('tables')).toBe('table')
    expect(ddlKind('routines', 'package_body')).toBe('package_body')
    expect(ddlKind('routines')).toBeNull()
    expect(ddlKind('synonyms')).toBe('synonym')
  })

  it('视图只能生成 SELECT', () => {
    expect(generateMenu('view', []).map((i) => i.key)).toEqual(['gen:select'])
  })

  it('关联查询：加载中、没有外键时置灰', () => {
    const loading = generateMenu('table', undefined).at(-1)!
    expect(loading.disabled).toBe(false)
    expect(loading.children).toEqual([{ key: 'gen:loading', label: '加载中…', disabled: true }])
    const none = generateMenu('table', []).at(-1)!
    expect(none.disabled).toBe(true)
  })

  it('关联查询：按外键列出两个方向；只有被引用时"全部外键"置灰', () => {
    const rels = [rel('in', 'order_items', ['order_id']), rel('in', 'refunds', ['order_id'])]
    const join = generateMenu('table', rels).at(-1)!
    expect(join.children!.map((c) => [c.label, c.disabled ?? false])).toEqual([
      ['全部外键', true],
      ['← order_items (order_id)', false],
      ['← refunds (order_id)', false],
    ])
    const out = generateMenu('table', [rel('out', 'users', ['user_id'])]).at(-1)!
    expect(out.children![0].disabled).toBe(false)
    expect(out.children![1].label).toBe('→ users (user_id)')
  })

  it('由菜单键取回生成的种类', () => {
    const rels = [rel('out', 'users', ['user_id'])]
    expect(generateChoice('gen:update', rels)).toEqual({ kind: 'update' })
    expect(generateChoice('gen:join', rels)).toEqual({ kind: 'join' })
    expect(generateChoice('gen:join:0', rels)).toEqual({ kind: 'join', relation: rels[0] })
    expect(generateChoice('gen:join:5', rels)).toBeNull()
    expect(generateChoice('refresh', rels)).toBeNull()
  })
})
