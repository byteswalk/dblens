import { describe, expect, it } from 'vitest'

import type { Profile } from '../../shared/api'
import i18n from '../../shared/i18n'
import {
  DRIVER_IDS,
  databaseLabel,
  driverLabel,
  emptyForm,
  fromProfile,
  profileKind,
  serverNotes,
  serverText,
  switchDriver,
  switchProduct,
  toProfileInput,
  validate,
} from './form'

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
      product: 'postgres',
    })
    const o = toProfileInput({ ...emptyForm('oracle'), name: 'o', user: 'u', oracleSid: 'ORCL', tls: 'require' })
    expect(o.options).toEqual({ sid: 'ORCL' })
  })

  it('SQL Server：认证方式、实例名、信任证书放进 options，当前 Windows 用户不要求用户名（M6a）', () => {
    const f = { ...switchProduct(emptyForm(), 'sqlserver'), name: 'ms', sqlAuth: 'windows' as const, user: 'HOST\\u', instance: 'SQLEXPRESS', trustCert: true, tls: 'require' as const }
    expect(f.port).toBe(1433)
    const input = toProfileInput(f)
    expect(input.options).toEqual({ auth: 'windows', instance: 'SQLEXPRESS', trust_server_certificate: 'true', tls: 'require' })
    const back = fromProfile({ id: 'x', ...input }, false)
    expect([back.sqlAuth, back.instance, back.trustCert, back.extra]).toEqual(['windows', 'SQLEXPRESS', true, ''])
    const integrated = { ...f, sqlAuth: 'integrated' as const, user: '' }
    expect(validate(integrated).user).toBeUndefined()
    expect(validate({ ...f, user: '' }).user).toBeDefined()
    expect(toProfileInput({ ...f, sqlAuth: 'sql' }).options?.auth).toBeUndefined()
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

  it('按产品选：驱动和默认端口跟着换，手改过的端口保留；配置记下产品，旧配置按驱动还原', () => {
    let f = switchProduct(emptyForm(), 'tidb')
    expect([f.driver, f.port]).toEqual(['mysql', 4000])
    f = switchProduct(f, 'kingbase')
    expect([f.driver, f.port]).toEqual(['postgres', 54321])
    f = switchProduct({ ...f, port: 6000 }, 'opengauss')
    expect([f.driver, f.port, toProfileInput(f).product]).toEqual(['postgres', 6000, 'opengauss'])
    const p = { id: 'p', name: 'n', driver: 'mysql' as const, host: 'h', port: 2881, user: 'u' }
    expect(fromProfile({ ...p, product: 'oceanbase' }, false).product).toBe('oceanbase')
    expect(fromProfile(p, false).product).toBe('mysql')
    // 与驱动对不上的值按驱动处理
    expect(fromProfile({ ...p, product: 'kingbase' }, false).product).toBe('mysql')
  })

  it('连接列表中的名称：连接过按变体，没连接过按选的产品，标准库和没选时按驱动', () => {
    expect(profileKind({ driver: 'postgres', variant: 'kingbase', product: 'opengauss' })).toEqual({ label: 'KingbaseES', icon: 'kingbase' })
    expect(profileKind({ driver: 'postgres', product: 'kingbase' })).toEqual({ label: '人大金仓', icon: 'kingbase' })
    expect(profileKind({ driver: 'postgres', product: 'postgres' })).toEqual({ label: 'PostgreSQL 系', icon: null })
    expect(profileKind({ driver: 'mysql' })).toEqual({ label: 'MySQL 系', icon: null })
  })

  it('测试连接的说明：兼容模式、未适配的模式、所选产品与识别结果不一致', () => {
    const s = { product: 'PostgreSQL', version: 'v', variant: 'kingbase', variant_name: '人大金仓', version_code: 120100 }
    const oracle = { ...s, compat: 'oracle' as const, mode: 'oracle', unadapted_mode: false }
    expect(serverNotes(oracle, 'kingbase')).toEqual([{ text: expect.stringContaining('兼容模式：oracle'), warn: false }])
    // 选标准库而识别出兼容库不算不一致
    expect(serverNotes(oracle, 'postgres')).toHaveLength(1)
    const b = { ...s, variant: 'opengauss', variant_name: 'openGauss', compat: 'native' as const, mode: 'B', unadapted_mode: true }
    expect(serverNotes(b, 'kingbase').map((n) => n.warn)).toEqual([true, true])
    expect(serverNotes(b, 'kingbase')[1].text).toBe('选的是 人大金仓，实际识别为 openGauss，按识别结果处理')
    const pg = { ...s, variant: 'postgres', variant_name: 'PostgreSQL', compat: 'native' as const, mode: null, unadapted_mode: false }
    expect(serverNotes(pg, 'postgres')).toEqual([])
  })

  it('服务器版本说明不重复产品名', () => {
    const s = (product: string, version: string) => ({
      product,
      version,
      variant: 'x',
      variant_name: 'x',
      version_code: null,
      compat: 'native' as const,
      mode: null,
      unadapted_mode: false,
    })
    expect(serverText(s('PostgreSQL', 'PostgreSQL 18.6 on x86_64'))).toBe('PostgreSQL 18.6 on x86_64')
    expect(serverText(s('MySQL', '9.7.4'))).toBe('MySQL 9.7.4')
  })

  it('驱动名称、字段名、校验信息按调用时的界面语言翻译', async () => {
    expect(DRIVER_IDS.map(driverLabel)).toEqual(['MySQL 系', 'PostgreSQL 系', 'Oracle', '达梦 DM', 'SQL Server', 'GBase 8s'])
    try {
      await i18n.changeLanguage('en')
      expect(DRIVER_IDS.map(driverLabel)).toEqual(['MySQL family', 'PostgreSQL family', 'Oracle', 'DM (Dameng)', 'SQL Server', 'GBase 8s'])
      expect(databaseLabel('oracle')).toBe('Service Name')
      expect(validate(emptyForm()).name).toBe('Enter a connection name')
      expect(validate({ ...emptyForm(), extra: 'bad' }).extra).toBe('Unrecognized line: bad (expected key=value)')
    } finally {
      await i18n.changeLanguage('zh-CN')
    }
  })
})

describe('GBase 8s', () => {
  const gbase = { ...emptyForm(), name: 'g', host: 'h', user: 'u', product: 'gbase8s' as const, driver: 'informix' as const }

  it('服务器名和 DB_LOCALE 进入连接选项', () => {
    const input = toProfileInput({ ...gbase, serverName: ' gbase01 ', dbLocale: '' })
    expect(input.options).toMatchObject({ server_name: 'gbase01' })
    expect(input.options).not.toHaveProperty('db_locale')
  })

  it('必须填服务器名', () => {
    expect(validate({ ...gbase, serverName: '' }).serverName).toBeTruthy()
    expect(validate({ ...gbase, serverName: 'gbase01' }).serverName).toBeUndefined()
  })
})
