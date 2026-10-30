// 数据库图标（M1i 设计文档第 6 节）：simple-icons 有的用它的官方 logo（CC0），
// 没有的用 dbIconArt.ts 中的官方标识，都没有矢量图的才用品牌主色加字标。图标只用来标识连接类型。

import { siMariadb, siMysql, siPostgresql, siTidb } from 'simple-icons'

import type { DriverId } from './api'
import { ART_ICONS, type ArtIcon } from './dbIconArt'

export type DbIconDef =
  /** 官方 logo：24×24 的 SVG 路径。`dark` 是暗色主题下的颜色（品牌色太深时提亮）。 */
  | { kind: 'logo'; path: string; color: string; dark?: string }
  /** 官方标识：多色，自带颜色。 */
  | ({ kind: 'art' } & ArtIcon)
  /** 字标：圆角方形底板上的缩写字母。 */
  | { kind: 'mark'; text: string; color: string }

const logo = (icon: { path: string; hex: string }, dark?: string): DbIconDef => ({
  kind: 'logo',
  path: icon.path,
  color: `#${icon.hex}`,
  dark,
})
const mark = (text: string, color: string): DbIconDef => ({ kind: 'mark', text, color })
const art = (icon: ArtIcon): DbIconDef => ({ kind: 'art', ...icon })

/** 按变体（后端 `Variant` 的序列化名称）。 */
export const VARIANT_ICONS: Record<string, DbIconDef> = {
  mysql: logo(siMysql, '#6ba6d0'),
  mariadb: logo(siMariadb, '#c49a6c'),
  tidb: logo(siTidb),
  oceanbase: art(ART_ICONS.oceanbase),
  polardb: mark('PDB', '#ff6a00'),
  postgres: logo(siPostgresql, '#7f9cf0'),
  kingbase: art(ART_ICONS.kingbase),
  opengauss: art(ART_ICONS.opengauss),
  gaussdb: mark('GS', '#c7000b'),
  highgo: mark('HG', '#0091d5'),
  oracle: art(ART_ICONS.oracle),
  dameng: art(ART_ICONS.dameng),
  sqlserver: art(ART_ICONS.sqlserver),
}

/** 还没连接过、不知道具体变体时，按驱动大类。 */
const DRIVER_ICONS: Record<DriverId, string> = {
  mysql: 'mysql',
  postgres: 'postgres',
  oracle: 'oracle',
  dameng: 'dameng',
  sqlserver: 'sqlserver',
}

export function dbIcon(driver: DriverId, variant?: string | null): DbIconDef {
  return (variant && VARIANT_ICONS[variant]) || VARIANT_ICONS[DRIVER_ICONS[driver]]
}
