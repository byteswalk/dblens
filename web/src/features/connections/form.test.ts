import { describe, expect, it } from 'vitest'

import type { Profile } from '../../shared/api'
import { emptyForm, fromProfile, serverText, switchDriver, toProfileInput, validate } from './form'

describe('连接表单', () => {
  it('换驱动时跟着换默认端口，手改过的端口保留；不支持 TLS 的驱动关掉 TLS', () => {
    let f = emptyForm('mysql')
    f = switchDriver({ ...f, tls: 'require' }, 'postgres')
    expect([f.port, f.tls]).toEqual([5432, 'require'])
    f = switchDriver({ ...f, port: 15432 }, 'oracle')
    expect([f.port, f.tls]).toEqual([15432, 'disable'])
  })

  it('转成连接配置：TLS、Oracle SID、其他参数放进 options，空值转 null', () => {
    const f = {
      ...emptyForm('postgres'),
      name: ' dev-pg ',
      host: 'db.local',
      user: 'app',
      tls: 'verify' as const,
      tlsCa: 'C:\\ca.pem',
      extra: 'application_name=dblens\n\n',
    }
    expect(toProfileInput(f)).toEqual({
      name: 'dev-pg',
      driver: 'postgres',
      host: 'db.local',
      port: 5432,
      user: 'app',
      database: null,
      options: { application_name: 'dblens', tls: 'verify', tls_ca: 'C:\\ca.pem' },
      environment: 'none',
      color: null,
      connect_timeout_secs: 10,
    })
    const o = toProfileInput({ ...emptyForm('oracle'), name: 'o', user: 'u', oracleSid: 'ORCL', tls: 'require' })
    expect(o.options).toEqual({ sid: 'ORCL' })
  })

  it('从已保存的配置还原表单，已知选项拆回各自的字段', () => {
    const p: Profile = {
      id: 'p1',
      name: 'prod',
      driver: 'mysql',
      host: 'h',
      port: 3307,
      user: 'u',
      database: 'shop',
      options: { tls: 'require', charset: 'utf8mb4' },
      environment: 'prod',
      color: null,
      connect_timeout_secs: 20,
    }
    const f = fromProfile(p, true)
    expect([f.tls, f.extra, f.environment, f.savePassword, f.password, f.timeoutSecs]).toEqual([
      'require',
      'charset=utf8mb4',
      'prod',
      true,
      '',
      20,
    ])
    expect(toProfileInput(f)).toMatchObject({ options: { tls: 'require', charset: 'utf8mb4' }, database: 'shop' })
  })

  it('校验必填项、端口、Oracle 服务名、参数格式', () => {
    expect(Object.keys(validate(emptyForm()))).toEqual(['name', 'user'])
    const ok = { ...emptyForm('oracle'), name: 'o', user: 'u', database: 'FREEPDB1' }
    expect(validate(ok)).toEqual({})
    expect(validate({ ...ok, database: '' }).database).toBe('填写服务名或 SID')
    expect(validate({ ...ok, port: 70000 }).port).toBeDefined()
    expect(validate({ ...ok, extra: 'a=1\nbad' }).extra).toContain('bad')
  })

  it('服务器版本说明不重复产品名', () => {
    const s = (product: string, version: string) => ({ product, version, variant: 'x', variant_name: 'x', version_code: null })
    expect(serverText(s('PostgreSQL', 'PostgreSQL 18.6 on x86_64'))).toBe('PostgreSQL 18.6 on x86_64')
    expect(serverText(s('MySQL', '9.7.4'))).toBe('MySQL 9.7.4')
  })
})
