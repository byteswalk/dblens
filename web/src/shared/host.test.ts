import { describe, expect, it } from 'vitest'
import { withoutHidden } from './host'

describe('withoutHidden', () => {
  const run = (items: string[], hide: string[]) =>
    withoutHidden(items, (i) => !hide.includes(i), (i) => i === '-')

  it('隐藏项目并收拢分隔项', () => {
    expect(run(['a', '-', 'b', '-', 'c'], ['b'])).toEqual(['a', '-', 'c'])
    expect(run(['a', '-', 'b'], ['b'])).toEqual(['a'])
    expect(run(['a', '-', 'b'], ['a'])).toEqual(['b'])
  })
  it('没有隐藏时原样返回', () => {
    expect(run(['a', '-', 'b'], [])).toEqual(['a', '-', 'b'])
  })
})
