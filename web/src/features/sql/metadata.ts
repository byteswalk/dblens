// 补全用的元数据（M1b2 设计文档第 4 节）：schema、对象、表的列、用户函数。
// - 放在 react-query 缓存里，键以 ['meta', 连接] 开头；对象树刷新时一并失效；
// - 按需取：表的列第一次需要时才取；
// - 补全不长时间等待：超过 waitMs 先返回空，请求继续在后台完成，下次补全时就有了；
// - 取不到（出错、密码库锁定）时当作空，不弹错误。

import type { QueryClient } from '@tanstack/react-query'

import { type Api, type DriverId, joinCatalog } from '../../shared/api'

export interface MetaObject {
  name: string
  kind: 'table' | 'view' | 'synonym' | 'sequence'
  comment?: string | null
}

export interface MetaColumn {
  name: string
  type: string
  comment?: string | null
}

export interface MetaReader {
  schemas(): Promise<string[]>
  objects(schema: string): Promise<MetaObject[]>
  columns(schema: string, table: string): Promise<MetaColumn[]>
  functions(schema: string): Promise<string[]>
}

export type MetaApi = Pick<Api, 'schemas' | 'objects' | 'table' | 'synonyms' | 'sequences' | 'routines'>

/** 某个连接（或其中一个 schema）的补全元数据的查询键前缀，对象树刷新时用来失效。 */
export const metaKey = (profileId: string, schema?: string) => (schema === undefined ? ['meta', profileId] : ['meta', profileId, schema])

/** 等 p 最多 ms 毫秒；超时或出错时得到 fallback（p 继续在后台完成）。 */
function within<T>(p: Promise<T>, ms: number, fallback: T): Promise<T> {
  return new Promise((resolve) => {
    const timer = setTimeout(() => resolve(fallback), ms)
    p.then(
      (v) => {
        clearTimeout(timer)
        resolve(v)
      },
      () => {
        clearTimeout(timer)
        resolve(fallback)
      },
    )
  })
}

/**
 * `catalog`：SQL Server 会话的当前库。读取的架构都在这个库中（与对象树的合成 schema 相同，缓存也共用）。
 */
export function metaReader(
  qc: QueryClient,
  api: MetaApi,
  profileId: string,
  driver: DriverId,
  catalog?: string | null,
  waitMs = 1500,
): MetaReader {
  const get = <T>(key: unknown[], fn: () => Promise<T>, fallback: T) =>
    within(qc.fetchQuery({ queryKey: key, queryFn: fn, staleTime: Infinity }), waitMs, fallback)
  const withSynonyms = driver === 'oracle' || driver === 'dameng' || driver === 'sqlserver'
  const inner = { schemas: () => get([...metaKey(profileId), '#schemas', catalog ?? ''], async () => (await api.schemas(profileId, undefined, catalog ?? undefined)).map((s) => s.name), []),
    objects: (schema: string) =>
      get(
        [...metaKey(profileId, schema), 'objects'],
        async () => {
          const [tables, views, synonyms, sequences] = await Promise.all([
            api.objects(profileId, schema, 'tables'),
            api.objects(profileId, schema, 'views'),
            withSynonyms ? api.synonyms(profileId, schema) : Promise.resolve([]),
            withSynonyms ? api.sequences(profileId, schema) : Promise.resolve([]),
          ])
          return [
            ...tables.map((o): MetaObject => ({ name: o.name, kind: 'table', comment: o.comment })),
            ...views.map((o): MetaObject => ({ name: o.name, kind: 'view', comment: o.comment })),
            ...synonyms.map((o): MetaObject => ({ name: o.name, kind: 'synonym' })),
            ...sequences.map((o): MetaObject => ({ name: o.name, kind: 'sequence' })),
          ]
        },
        [],
      ),
    columns: (schema: string, table: string) =>
      get(
        [...metaKey(profileId, schema), 'columns', table],
        async () =>
          ((await api.table(profileId, schema, table))?.columns ?? []).map((c) => ({
            name: c.name,
            type: c.data_type.native,
            comment: c.comment,
          })),
        [],
      ),
    functions: (schema: string) =>
      get(
        [...metaKey(profileId, schema), 'functions'],
        async () => (await api.routines(profileId, schema)).filter((r) => r.kind === 'function').map((r) => r.name),
        [],
      ),
  }
  if (driver !== 'sqlserver' || !catalog) return inner
  // SQL Server：补全按架构名取元数据，请求时带上当前库
  const at = (schema: string) => joinCatalog(catalog, schema)
  return {
    schemas: inner.schemas,
    objects: (schema) => inner.objects(at(schema)),
    columns: (schema, table) => inner.columns(at(schema), table),
    functions: (schema) => inner.functions(at(schema)),
  }
}

/** 按名称找：先找完全相同的，再不分大小写（语句里写的 emp 对应 Oracle 中的 EMP）。 */
export function findName<T extends { name: string } | string>(items: T[], name: string): T | undefined {
  const n = (x: T) => (typeof x === 'string' ? x : x.name)
  return items.find((x) => n(x) === name) ?? items.find((x) => n(x).toUpperCase() === name.toUpperCase())
}

/**
 * 补全用的当前 schema：MySQL 系是连接配置里的库，PG 系是 public，Oracle 和达梦是用户名（大写），
 * SQL Server 是 dbo（会话的当前库另作 `metaReader` 的 catalog）。
 */
export function defaultSchema(driver: DriverId, user: string, database?: string | null): string | null {
  switch (driver) {
    case 'mysql':
      return database || null
    case 'postgres':
      return 'public'
    case 'oracle':
    case 'dameng':
      return user.toUpperCase()
    case 'sqlserver':
      return 'dbo'
  }
}
