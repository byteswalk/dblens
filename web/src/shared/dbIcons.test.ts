import { describe, expect, it } from 'vitest'

import { VARIANT_ICONS, dbIcon } from './dbIcons'

// 后端 Variant 的全部序列化名称（crates/dblens-dialect/src/variant.rs）
const VARIANTS = [
  'mysql',
  'mariadb',
  'tidb',
  'oceanbase',
  'polardb',
  'postgres',
  'kingbase',
  'opengauss',
  'gaussdb',
  'highgo',
  'oracle',
  'dameng',
]

describe('数据库图标', () => {
  it('每个变体都有图标：开源 logo 是 SVG 路径，其余是字标', () => {
    expect(Object.keys(VARIANT_ICONS).sort()).toEqual([...VARIANTS].sort())
    for (const v of ['mysql', 'mariadb', 'tidb', 'postgres']) expect(VARIANT_ICONS[v].kind).toBe('logo')
    expect(dbIcon('dameng', 'dameng')).toEqual({ kind: 'mark', text: 'DM', color: '#c8102e' })
    expect(dbIcon('postgres', 'kingbase')).toMatchObject({ kind: 'mark', text: 'KES' })
  })

  it('不知道变体时按驱动大类，认不出的变体也按驱动', () => {
    expect(dbIcon('postgres')).toBe(VARIANT_ICONS.postgres)
    expect(dbIcon('mysql', null)).toBe(VARIANT_ICONS.mysql)
    expect(dbIcon('oracle', 'something-new')).toBe(VARIANT_ICONS.oracle)
  })

  it('品牌色太深的 logo 在暗色主题下有提亮的颜色', () => {
    expect(VARIANT_ICONS.mariadb).toMatchObject({ kind: 'logo', color: '#003545', dark: '#c49a6c' })
  })
})
