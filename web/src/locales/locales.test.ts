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

/** 英文的单复数用 i18next 的 `_one` / `_other` 后缀，中文没有单复数：比较时去掉后缀。 */
const PLURAL = /_(zero|one|two|few|many|other)$/
const base = (key: string) => key.replace(PLURAL, '')

describe('词条', () => {
  const zh = flatten(zhCN)
  const e = flatten(en)

  it('中英文的键完全一致（不计单复数后缀）', () => {
    expect([...new Set(Object.keys(e).map(base))].sort()).toEqual(Object.keys(zh).sort())
  })

  it('每个词条的插值变量一致，且都不为空；有单复数的词条带 count', () => {
    for (const [key, text] of Object.entries(e)) {
      const zhText = zh[base(key)]
      expect(text, key).not.toBe('')
      expect(zhText, key).not.toBe('')
      expect(vars(text), key).toEqual(vars(zhText ?? ''))
      if (PLURAL.test(key)) expect(vars(text), key).toContain('count')
    }
  })
})
