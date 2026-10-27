import { describe, expect, it } from 'vitest'

import type { OpsQuery, Profile } from '../../shared/api'
import { groupByCategory, pickProfile } from './ops'

const q = (key: string, category: OpsQuery['category']): OpsQuery => ({ key, name: key, category, sql: 'SELECT 1' })
const profile = (id: string) => ({ id }) as Profile

describe('运维查询侧栏', () => {
  it('按固定的分类顺序分组，组内保持原顺序，空分类不出现', () => {
    const groups = groupByCategory([q('slow', 'sql'), q('sessions', 'session'), q('running', 'session'), q('size', 'storage')])
    expect(groups.map((g) => [g.category, g.items.map((i) => i.key)])).toEqual([
      ['session', ['sessions', 'running']],
      ['storage', ['size']],
      ['sql', ['slow']],
    ])
  })

  it('连接：依次取侧栏选的、对象树选中的、当前标签的，已删除的跳过，最后取第一个', () => {
    const ps = [profile('a'), profile('b'), profile('c')]
    expect(pickProfile(ps, null, 'b', 'c')).toBe('b')
    expect(pickProfile(ps, 'gone', undefined, 'c')).toBe('c')
    expect(pickProfile(ps, null, null, null)).toBe('a')
    expect(pickProfile([], 'a')).toBeNull()
  })
})
