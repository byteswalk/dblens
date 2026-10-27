import { describe, expect, it } from 'vitest'

import { nextOrder, sortedTitle } from './sort'

describe('数据标签的排序', () => {
  it('同一列依次切换升序、降序、不排序', () => {
    const asc = nextOrder(null, 'a')
    expect(asc).toEqual({ column: 'a', desc: false })
    const desc = nextOrder(asc, 'a')
    expect(desc).toEqual({ column: 'a', desc: true })
    expect(nextOrder(desc, 'a')).toBeNull()
  })

  it('换一列时从升序开始', () => {
    expect(nextOrder({ column: 'a', desc: true }, 'b')).toEqual({ column: 'b', desc: false })
    expect(nextOrder(undefined, 'b')).toEqual({ column: 'b', desc: false })
  })

  it('排序的列头加箭头', () => {
    expect(sortedTitle('a', { column: 'a', desc: false })).toBe('a ↑')
    expect(sortedTitle('a', { column: 'a', desc: true })).toBe('a ↓')
    expect(sortedTitle('b', { column: 'a', desc: true })).toBe('b')
    expect(sortedTitle('b', null)).toBe('b')
  })
})
