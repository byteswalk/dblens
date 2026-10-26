// 数据库图标（M1i 设计文档第 6 节）：有开源矢量图的用 simple-icons 的官方 logo（CC0），
// 其余用品牌主色加统一字标，不复刻对方的图形 logo。图标只用来标识连接类型。

import { siMariadb, siMysql, siPostgresql, siTidb } from 'simple-icons'

import type { DriverId } from './api'

export type DbIconDef =
  /** 官方 logo：24×24 的 SVG 路径。`dark` 是暗色主题下的颜色（品牌色太深时提亮）。 */
  | { kind: 'logo'; path: string; color: string; dark?: string }
  /** 字标：圆角方形底板上的缩写字母。 */
  | { kind: 'mark'; text: string; color: string }

const logo = (icon: { path: string; hex: string }, dark?: string): DbIconDef => ({
  kind: 'logo',
  path: icon.path,
  color: `#${icon.hex}`,
  dark,
})
const mark = (text: string, color: string): DbIconDef => ({ kind: 'mark', text, color })

/** 按变体（后端 `Variant` 的序列化名称）。 */
export const VARIANT_ICONS: Record<string, DbIconDef> = {
  mysql: logo(siMysql),
  mariadb: logo(siMariadb, '#c49a6c'),
  tidb: logo(siTidb),
  oceanbase: mark('OB', '#006aff'),
  polardb: mark('PDB', '#ff6a00'),
  postgres: logo(siPostgresql),
  kingbase: mark('KES', '#1e64c8'),
  opengauss: mark('oG', '#5b3cc4'),
  gaussdb: mark('GS', '#c7000b'),
  highgo: mark('HG', '#0091d5'),
  oracle: mark('ORA', '#c74634'),
  dameng: mark('DM', '#c8102e'),
}

/** 还没连接过、不知道具体变体时，按驱动大类。 */
const DRIVER_ICONS: Record<DriverId, string> = {
  mysql: 'mysql',
  postgres: 'postgres',
  oracle: 'oracle',
  dameng: 'dameng',
}

export function dbIcon(driver: DriverId, variant?: string | null): DbIconDef {
  return (variant && VARIANT_ICONS[variant]) || VARIANT_ICONS[DRIVER_ICONS[driver]]
}
