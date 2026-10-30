// 连接下可选的 schema（比对、迁移、反向生成模型的下拉框）。SQL Server 有库这一层（M6b）：列出每个用户库的架构，
// 值是合成的 schema（`joinCatalog`），标签写成 `库.架构`；系统库和系统架构不列出。

import { type Api, type DriverId, type Profile, joinCatalog } from './api'

export interface SchemaOption {
  value: string
  label: string
}

/** 有“库”这一层的驱动：连接 → 库 → 架构或属主 → 对象。 */
export function hasCatalogs(driver: DriverId): boolean {
  return driver === 'sqlserver' || driver === 'informix'
}

export async function schemaOptions(api: Api, profile: Profile): Promise<SchemaOption[]> {
  if (!hasCatalogs(profile.driver)) {
    return (await api.schemas(profile.id)).map((s) => ({ value: s.name, label: s.name }))
  }
  const catalogs = (await api.catalogs(profile.id)).filter((c) => !c.is_system)
  const lists = await Promise.all(
    catalogs.map(async (c) =>
      (await api.schemas(profile.id, undefined, c.name))
        .filter((s) => !s.is_system)
        .map((s) => ({ value: joinCatalog(c.name, s.name), label: `${c.name}.${s.name}` })),
    ),
  )
  return lists.flat()
}
