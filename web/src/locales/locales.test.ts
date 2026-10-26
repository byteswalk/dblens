import { describe, expect, it } from 'vitest'

import { en, zhCN } from '.'

/** 把嵌套的词条展开成 键 -> 文字。 */
function flatten(obj: object, prefix = ''): Record<string, string> {
  const out: Record<string, string> = {}
  for (const [k, v] of Object.entries(obj)) {
    const key = prefix ? `${prefix}.${k}` : k
    if (typeof v === 'string') out[key] = v
    else Object.assign(out, flatten(v as object, key))
  }
  return out
}

const vars = (s: string) => [...s.matchAll(/\{\{\s*(\w+)/g)].map((m) => m[1]).sort()

describe('词条', () => {
  const zh = flatten(zhCN)
  const e = flatten(en)

  it('中英文的键完全一致', () => {
    expect(Object.keys(e).sort()).toEqual(Object.keys(zh).sort())
  })

  it('每个词条的插值变量一致，且都不为空', () => {
    for (const [key, text] of Object.entries(zh)) {
      expect(text, key).not.toBe('')
      expect(e[key], key).not.toBe('')
      expect(vars(e[key] ?? ''), key).toEqual(vars(text))
    }
  })
})
