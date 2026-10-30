// 新建连接时按产品选（国产库第一批设计 4.3 节）：每个产品对应一个驱动、一个默认端口。
// 选择记在连接配置的 `product` 上；连上后以识别出的变体为准。

import type { DriverId } from '../../shared/api'
import { t } from '../../shared/i18n'

export type ProductId =
  | 'mysql'
  | 'mariadb'
  | 'tidb'
  | 'oceanbase'
  | 'postgres'
  | 'kingbase'
  | 'opengauss'
  | 'gaussdb'
  | 'oracle'
  | 'dameng'
  | 'sqlserver'
  | 'gbase8s'

export interface Product {
  id: ProductId
  driver: DriverId
  port: number
}

export const PRODUCTS: Product[] = [
  { id: 'mysql', driver: 'mysql', port: 3306 },
  { id: 'mariadb', driver: 'mysql', port: 3306 },
  { id: 'tidb', driver: 'mysql', port: 4000 },
  { id: 'oceanbase', driver: 'mysql', port: 2881 },
  { id: 'postgres', driver: 'postgres', port: 5432 },
  { id: 'kingbase', driver: 'postgres', port: 54321 },
  { id: 'opengauss', driver: 'postgres', port: 5432 },
  { id: 'gaussdb', driver: 'postgres', port: 8000 },
  { id: 'oracle', driver: 'oracle', port: 1521 },
  { id: 'dameng', driver: 'dameng', port: 5236 },
  { id: 'sqlserver', driver: 'sqlserver', port: 1433 },
  { id: 'gbase8s', driver: 'informix', port: 9088 },
]

/** 界面上的分组：国产库一组，其他一组。 */
export const PRODUCT_GROUPS: { key: 'domestic' | 'other'; products: ProductId[] }[] = [
  { key: 'domestic', products: ['dameng', 'kingbase', 'gaussdb', 'opengauss', 'oceanbase', 'tidb', 'gbase8s'] },
  { key: 'other', products: ['mysql', 'mariadb', 'postgres', 'oracle', 'sqlserver'] },
]

export function product(id: ProductId): Product {
  return PRODUCTS.find((p) => p.id === id) ?? PRODUCTS[0]
}

/** 只有驱动、没有记下产品（较早保存的连接）时对应的产品。 */
export function baseProduct(driver: DriverId): ProductId {
  return driver === 'informix' ? 'gbase8s' : driver
}

/** 连接配置上记的产品；不认识的值按驱动处理。 */
export function productOf(driver: DriverId, saved?: string | null): ProductId {
  const p = PRODUCTS.find((x) => x.id === saved && x.driver === driver)
  return p ? p.id : baseProduct(driver)
}

export function productLabel(id: ProductId): string {
  return t(`connections.product.${id}.label`)
}

/** 产品的说明：默认端口以外的登录提示、需要的客户端库等；没有时为空串。 */
export function productNote(id: ProductId): string {
  return t(`connections.product.${id}.note`, { defaultValue: '' })
}
