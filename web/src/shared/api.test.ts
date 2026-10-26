import { describe, expect, it } from 'vitest'

import { ApiError, type FetchFn, createApi, errorText } from './api'
import i18n from './i18n'

function fakeFetch(status: number, body: unknown) {
  const calls: { url: string; init?: RequestInit }[] = []
  const fn: FetchFn = async (url, init) => {
    calls.push({ url, init })
    return new Response(body === null ? null : JSON.stringify(body), { status })
  }
  return { fn, calls }
}

describe('createApi', () => {
  it('带上令牌和 JSON 请求体', async () => {
    const { fn, calls } = fakeFetch(201, { id: 'p1', name: 'x' })
    const api = createApi('tok', fn)
    const p = await api.createProfile({ name: 'x', driver: 'mysql', host: 'h', port: 3306, user: 'u' }, 'pw')
    expect(p.id).toBe('p1')
    expect(calls[0].url).toBe('/api/profiles')
    expect(calls[0].init?.method).toBe('POST')
    expect(calls[0].init?.headers).toEqual({
      Authorization: 'Bearer tok',
      'Accept-Language': 'zh-CN',
      'Content-Type': 'application/json',
    })
    expect(JSON.parse(calls[0].init?.body as string)).toEqual({
      profile: { name: 'x', driver: 'mysql', host: 'h', port: 3306, user: 'u' },
      password: 'pw',
    })
  })

  it('204 没有响应体', async () => {
    const { fn, calls } = fakeFetch(204, null)
    await expect(createApi('tok', fn).deleteProfile('a/b')).resolves.toBeNull()
    expect(calls[0].url).toBe('/api/profiles/a%2Fb')
    expect(calls[0].init?.headers).toEqual({ Authorization: 'Bearer tok', 'Accept-Language': 'zh-CN' })
  })

  it('出错时抛出带错误对象的 ApiError', async () => {
    const { fn } = fakeFetch(422, { kind: 'sql', code: '1146', message: '表不存在', position: null })
    const err = await createApi('tok', fn).profiles().catch((e) => e)
    expect(err).toBeInstanceOf(ApiError)
    expect(err.status).toBe(422)
    expect(err.body.kind).toBe('sql')
    expect(err.message).toBe('SQL 错误 [1146]：表不存在')
  })

  it('WebSocket 地址带令牌和语言，https 页面用 wss', () => {
    const api = createApi('a b', fakeFetch(200, null).fn)
    expect(api.wsUrl(7, { protocol: 'http:', host: '127.0.0.1:8742' })).toBe(
      'ws://127.0.0.1:8742/api/sessions/7/ws?token=a%20b&lang=zh-CN',
    )
    expect(api.wsUrl(7, { protocol: 'https:', host: 'x' })).toBe('wss://x/api/sessions/7/ws?token=a%20b&lang=zh-CN')
  })

  it('请求和 WebSocket 都跟随界面语言', async () => {
    await i18n.changeLanguage('en')
    try {
      const { fn, calls } = fakeFetch(204, null)
      const api = createApi('tok', fn)
      await api.deleteProfile('x')
      expect((calls[0].init?.headers as Record<string, string>)['Accept-Language']).toBe('en')
      expect(api.wsUrl(7, { protocol: 'http:', host: 'h' })).toBe('ws://h/api/sessions/7/ws?token=tok&lang=en')
    } finally {
      await i18n.changeLanguage('zh-CN')
    }
  })
})

describe('errorText', () => {
  it('与服务端措辞一致', () => {
    expect(errorText({ kind: 'not_found', what: '连接配置 x' })).toBe('找不到连接配置 x')
    expect(errorText({ kind: 'locked' })).toBe('密码库已锁定，请先输入主密码')
    expect(errorText({ kind: 'weird', x: 1 } as never)).toBe('{"kind":"weird","x":1}')
  })

  it('类别前缀随界面语言', async () => {
    await i18n.changeLanguage('en')
    try {
      expect(errorText({ kind: 'not_found', what: 'connection profile x' })).toBe('Not found: connection profile x')
      expect(errorText({ kind: 'sql', code: '1146', message: 'm' })).toBe('SQL error [1146]: m')
    } finally {
      await i18n.changeLanguage('zh-CN')
    }
  })
})

describe('元数据与设置', () => {
  it('路径中的名字要编码，空过滤条件不带参数', async () => {
    const { fn, calls } = fakeFetch(200, [])
    const api = createApi('tok', fn)
    await api.schemas('p/1')
    await api.objects('p1', 'my schema', 'views', 'ord')
    await api.objects('p1', 'public', 'tables', '')
    await api.routines('p1', 'HR')
    expect(calls.map((c) => c.url)).toEqual([
      '/api/profiles/p%2F1/schemas',
      '/api/profiles/p1/schemas/my%20schema/objects?kind=views&filter=ord',
      '/api/profiles/p1/schemas/public/objects?kind=tables',
      '/api/profiles/p1/schemas/HR/routines',
    ])
  })

  it('table 取单表明细，表不存在时返回 null', async () => {
    const { fn, calls } = fakeFetch(200, [{ name: 'orders', comment: null, columns: [], primary_key: null }])
    expect((await createApi('tok', fn).table('p1', 'public', 'orders'))?.name).toBe('orders')
    expect(calls[0].url).toBe('/api/profiles/p1/schemas/public/tables?table=orders')
    expect(await createApi('tok', fakeFetch(200, []).fn).table('p1', 'public', 'x')).toBeNull()
  })

  it('设置的读写', async () => {
    const { fn, calls } = fakeFetch(204, null)
    await createApi('tok', fn).putSetting('ui.theme', 'dark')
    expect(calls[0].url).toBe('/api/settings/ui.theme')
    expect(calls[0].init?.method).toBe('PUT')
    expect(JSON.parse(calls[0].init?.body as string)).toEqual({ value: 'dark' })
  })
})
