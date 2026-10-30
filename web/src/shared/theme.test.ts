import { describe, expect, it } from 'vitest'

import { parseThemePref, resolveTheme } from './theme'

describe('主题', () => {
  it('跟随系统时取系统的亮暗，固定时不受系统影响', () => {
    expect(resolveTheme('system', true)).toBe('dark')
    expect(resolveTheme('system', false)).toBe('light')
    expect(resolveTheme('light', true)).toBe('light')
    expect(resolveTheme('dark', false)).toBe('dark')
  })

  it('设置值不认识或缺失时按跟随系统处理', () => {
    expect(parseThemePref('dark')).toBe('dark')
    expect(parseThemePref(undefined)).toBe('system')
    expect(parseThemePref('blue')).toBe('system')
    expect(parseThemePref('slate')).toBe('slate')
    expect(parseThemePref('rose')).toBe('rose')
  })
})
