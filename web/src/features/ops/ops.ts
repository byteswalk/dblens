// 运维查询侧栏的纯逻辑（M1c3 设计文档 5.3 节）：按分类分组、选用哪个连接。

import type { OpsQuery, Profile } from '../../shared/api'

export const CATEGORIES: OpsQuery['category'][] = ['session', 'lock', 'storage', 'sql']

/** 按分类分组，分类按固定顺序，组内保持服务端给的顺序；空分类不出现。 */
export function groupByCategory(list: OpsQuery[]): { category: OpsQuery['category']; items: OpsQuery[] }[] {
  return CATEGORIES.map((category) => ({ category, items: list.filter((q) => q.category === category) })).filter(
    (g) => g.items.length > 0,
  )
}

/** 用哪个连接：用户在侧栏选的，否则对象树中选中的，否则当前标签的，否则第一个。已删除的不算。 */
export function pickProfile(
  profiles: Profile[],
  ...candidates: (string | null | undefined)[]
): string | null {
  for (const id of candidates) if (id && profiles.some((p) => p.id === id)) return id
  return profiles[0]?.id ?? null
}
