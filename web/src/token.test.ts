import { describe, expect, it } from 'vitest'

import { TOKEN_IN_HASH, TOKEN_KEY, type TokenHost, bootstrapToken } from './token'

function host(hash: string, stored: Record<string, string> = {}, injected?: string) {
  const calls: string[] = []
  const h: TokenHost = {
    __DBLENS__: injected ? { token: injected } : undefined,
    location: { hash, pathname: '/query', search: '?a=1' },
    history: { replaceState: (_d, _u, url) => calls.push(url) },
    sessionStorage: {
      getItem: (k) => stored[k] ?? null,
      setItem: (k, v) => {
        stored[k] = v
      },
    },
  }
  return { h, calls, stored }
}

describe('bootstrapToken', () => {
  it('优先使用桌面模式注入的令牌，不碰地址栏', () => {
    const { h, calls } = host('#token=fromhash', {}, 'injected')
    expect(bootstrapToken(h)).toBe('injected')
    expect(calls).toEqual([])
  })

  it('从 #token= 取出令牌，存起来并清掉地址栏中的令牌', () => {
    const { h, calls, stored } = host('#token=ab%2Fc')
    expect(bootstrapToken(h)).toBe('ab/c')
    expect(stored[TOKEN_KEY]).toBe('ab/c')
    expect(calls).toEqual(['/query?a=1'])
  })

  it('# 后面有其他参数时也能取到', () => {
    expect(bootstrapToken(host('#x=1&token=t2').h)).toBe('t2')
  })

  it('刷新后从 sessionStorage 取', () => {
    expect(bootstrapToken(host('', { [TOKEN_KEY]: 'saved' }).h)).toBe('saved')
  })

  it('都没有时返回 null', () => {
    expect(bootstrapToken(host('#other=1').h)).toBeNull()
  })
})

describe('TOKEN_IN_HASH', () => {
  it('只认 # 后面的 token 参数', () => {
    expect(TOKEN_IN_HASH.test('#token=abc')).toBe(true)
    expect(TOKEN_IN_HASH.test('#x=1&token=abc')).toBe(true)
    expect(TOKEN_IN_HASH.test('#tokenx=abc')).toBe(false)
    expect(TOKEN_IN_HASH.test('#token=')).toBe(false)
    expect(TOKEN_IN_HASH.test('')).toBe(false)
  })
})
