// 设计器选外键引用、选 schema 时的数据来源：连接的元数据，或模型中的表（M4a）。

import type { Api } from '../../shared/api'

export interface RefSource {
  /** 查询缓存的键：连接为连接 id，模型为 `model:<编号>`。 */
  key: string
  schemas(): Promise<string[]>
  tables(schema: string): Promise<string[]>
  columns(schema: string, table: string): Promise<string[]>
}

export function profileSource(api: Api, profileId: string): RefSource {
  return {
    key: profileId,
    schemas: async () => (await api.schemas(profileId)).map((s) => s.name),
    tables: async (schema) => (await api.objects(profileId, schema, 'tables')).map((o) => o.name),
    columns: async (schema, table) => (await api.table(profileId, schema, table))?.columns.map((c) => c.name) ?? [],
  }
}

/** 模型：schema 为模型信息中的和已有表文件的；文件有错的表不列出。 */
export function modelSource(api: Api, modelId: string): RefSource {
  return {
    key: `model:${modelId}`,
    schemas: async () => {
      const [m, tables] = await Promise.all([api.model(modelId), api.modelTables(modelId)])
      return [...new Set([...m.info.schemas, ...tables.map((t) => t.schema)])]
    },
    tables: async (schema) =>
      (await api.modelTables(modelId)).filter((t) => t.schema === schema && !t.error).map((t) => t.name),
    columns: async (schema, table) => {
      try {
        return (await api.modelTable(modelId, schema, table)).table.columns.map((c) => c.name)
      } catch {
        return []
      }
    },
  }
}
