// 账号与权限的纯逻辑（M9 设计文档第 8 节）。

import type { ObjectAccess, Principal, PrivilegeGrant } from '../../shared/api'

export type KindFilter = 'all' | 'user' | 'role'

export interface PrincipalFilter {
  text: string
  kind: KindFilter
  /** 是否列出内置账号和角色。 */
  builtin: boolean
}

export function filterPrincipals(list: Principal[], f: PrincipalFilter): Principal[] {
  const text = f.text.trim().toLowerCase()
  return list.filter((p) => {
    if (!f.builtin && p.builtin) return false
    if (f.kind === 'role' && p.kind !== 'role') return false
    if (f.kind === 'user' && p.kind === 'role') return false
    return !text || p.name.toLowerCase().includes(text) || (p.host ?? '').toLowerCase().includes(text)
  })
}

/** 授权作用的对象，全局范围时为空串。 */
export function targetLabel(g: PrivilegeGrant): string {
  const { database, schema, object, column } = g.target
  return [g.scope === 'database' ? database : null, schema, object, column].filter(Boolean).join('.')
}

export function pathLabel(via: string[], principals: Principal[]): string {
  return via.map((id) => principals.find((p) => p.id === id)?.name ?? id).join(' → ')
}

/** 反查结果按主体合并，保持出现顺序。 */
export function groupAccess(rows: ObjectAccess[]): { principal: string; items: ObjectAccess[] }[] {
  const out: { principal: string; items: ObjectAccess[] }[] = []
  for (const r of rows) {
    const g = out.find((x) => x.principal === r.principal)
    if (g) g.items.push(r)
    else out.push({ principal: r.principal, items: [r] })
  }
  return out
}
