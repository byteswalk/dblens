import { describe, expect, it } from 'vitest'

import type { EffectivePrivilege, ObjectAccess, Principal, PrivilegeGrant } from '../../shared/api'
import { hasCredentials, principalRef, randomPassword, withCascade, filterPrincipals, groupAccess, pathLabel, targetLabel } from './logic'

const p = (id: string, over: Partial<Principal> = {}): Principal => ({
  id, name: id, host: null, kind: 'user', level: 'server', can_login: true, locked: null, expires_at: null, builtin: false, attrs: [], ...over,
})
const g = (over: Partial<PrivilegeGrant> = {}): PrivilegeGrant => ({
  grantee: 'u', scope: 'object',
  target: { database: null, schema: 'hr', object_kind: 'table', object: 'emp', column: null },
  privilege: 'SELECT', effect: 'grant', grantable: false, grantor: null, ...over,
})

describe('filterPrincipals', () => {
  const list = [p('alice'), p('r', { kind: 'role' }), p('sys', { builtin: true })]
  it('默认不列内置账号', () => {
    expect(filterPrincipals(list, { text: '', kind: 'all', builtin: false }).map((x) => x.id)).toEqual(['alice', 'r'])
  })
  it('按种类和文字过滤，不区分大小写', () => {
    expect(filterPrincipals(list, { text: '', kind: 'role', builtin: true }).map((x) => x.id)).toEqual(['r'])
    expect(filterPrincipals(list, { text: 'ALI', kind: 'all', builtin: true }).map((x) => x.id)).toEqual(['alice'])
  })
  it('登录名算用户一类', () => {
    expect(filterPrincipals([p('l', { kind: 'login' })], { text: '', kind: 'user', builtin: false })).toHaveLength(1)
  })
  it('host 也能搜', () => {
    expect(filterPrincipals([p('a', { host: '10.0.%' })], { text: '10.0', kind: 'all', builtin: false })).toHaveLength(1)
  })
})

describe('targetLabel', () => {
  it('按范围写出对象', () => {
    expect(targetLabel(g())).toBe('hr.emp')
    expect(targetLabel(g({ scope: 'column', target: { ...g().target, column: 'id' } }))).toBe('hr.emp.id')
    expect(targetLabel(g({ scope: 'schema', target: { ...g().target, object: null } }))).toBe('hr')
    expect(targetLabel(g({ scope: 'database', target: { ...g().target, database: 'db', schema: null, object: null } }))).toBe('db')
    expect(targetLabel(g({ scope: 'global', target: { database: null, schema: null, object_kind: null, object: null, column: null } }))).toBe('')
  })
})

describe('pathLabel', () => {
  it('用主体名字连成路径，认不出的 id 原样显示', () => {
    expect(pathLabel(['a', 'b'], [p('a', { name: '甲' })])).toBe('甲 → b')
  })
})

describe('groupAccess', () => {
  const e = (privilege: string, state: EffectivePrivilege['state'] = 'allowed'): EffectivePrivilege => ({ grant: g({ privilege }), via: ['u'], state, notes: [] })
  it('按主体合并，保持出现顺序', () => {
    const rows: ObjectAccess[] = [
      { principal: 'u', privilege: e('SELECT') },
      { principal: 'v', privilege: e('INSERT') },
      { principal: 'u', privilege: e('UPDATE') },
    ]
    expect(groupAccess(rows).map((x) => [x.principal, x.items.length])).toEqual([['u', 2], ['v', 1]])
  })
})

describe('修改账号', () => {
  const base: Principal = { id: 'a', name: 'a', host: null, kind: 'user', level: 'server', can_login: true, locked: false, expires_at: null, builtin: false, attrs: [] }

  it('随机密码够长、四类字符都有、没有引号', () => {
    for (let i = 0; i < 50; i++) {
      const p = randomPassword()
      expect(p).toHaveLength(20)
      expect(p).toMatch(/[A-Z]/)
      expect(p).toMatch(/[a-z]/)
      expect(p).toMatch(/[0-9]/)
      expect(p).toMatch(/[!#%*+=_-]/)
      expect(p).not.toMatch(/['"\\]/)
    }
  })

  it('角色和数据库用户不能改密', () => {
    expect(hasCredentials(base)).toBe(true)
    expect(hasCredentials({ ...base, kind: 'login' })).toBe(true)
    expect(hasCredentials({ ...base, kind: 'role' })).toBe(false)
    expect(hasCredentials({ ...base, level: 'database' })).toBe(false)
  })

  it('级联只作用于删除', () => {
    const principal = principalRef(base)
    expect(withCascade({ op: 'drop', principal }, true)).toEqual({ op: 'drop', principal, cascade: true })
    const lock = { op: 'set_locked', principal, locked: true } as const
    expect(withCascade(lock, true)).toBe(lock)
  })
})
