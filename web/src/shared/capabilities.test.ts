import { describe, expect, it } from 'vitest'
import { STANDALONE, hasFeature } from './capabilities'

describe('capabilities', () => {
  it('单机版没有审计版功能', () => {
    expect(STANDALONE.edition).toBe('standalone')
    expect(hasFeature(STANDALONE, 'tickets')).toBe(false)
  })
  it('按名称判断功能', () => {
    expect(hasFeature({ edition: 'audit', features: ['tickets'] }, 'tickets')).toBe(true)
  })
})
