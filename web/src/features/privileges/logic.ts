// 账号与权限的纯逻辑（M9 设计文档第 8 节）。

import type { ObjectAccess, Principal, PrincipalRef, PrivChange, PrivilegeGrant } from '../../shared/api'

export type KindFilter = 'all' | 'user' | 'role'

export function principalRef(p: Principal): PrincipalRef {
  return { name: p.name, host: p.host, kind: p.kind, level: p.level }
}

/** 这个主体能不能改密、锁定：角色不能；SQL Server 的数据库用户要改对应的登录名。 */
export function hasCredentials(p: Principal): boolean {
  return p.kind === 'login' || (p.kind === 'user' && p.level === 'server')
}

export function withCascade(change: PrivChange, cascade: boolean): PrivChange {
  return change.op === 'drop' ? { ...change, cascade } : change
}

/** 本地生成的随机密码：大小写字母、数字、符号各至少一个，不含引号和反斜杠。 */
export function randomPassword(length = 20): string {
  const sets = ['ABCDEFGHJKLMNPQRSTUVWXYZ', 'abcdefghijkmnpqrstuvwxyz', '23456789', '!#%*+-=_']
  const all = sets.join('')
  const pick = (from: string) => {
    const r = new Uint32Array(1)
    crypto.getRandomValues(r)
    return from[r[0] % from.length]
  }
  const chars = [...sets.map(pick), ...Array.from({ length: length - sets.length }, () => pick(all))]
  for (let i = chars.length - 1; i > 0; i--) {
    const r = new Uint32Array(1)
    crypto.getRandomValues(r)
    const j = r[0] % (i + 1)
    ;[chars[i], chars[j]] = [chars[j], chars[i]]
  }
  return chars.join('')
}

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
