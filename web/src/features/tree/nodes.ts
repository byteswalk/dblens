// 对象树的节点结构（设计文档 7.3 节）：连接 → schema → 分类（表、视图……）→ 对象 →（表的）列。
// 懒加载：展开时才向后端取下一层。这里只放纯逻辑，单独测试。

import type { DriverId, ObjectKind, RoutineInfo } from '../../shared/api'
import { t } from '../../shared/i18n'

export type CategoryKind = ObjectKind | 'routines' | 'sequences' | 'synonyms'

export interface Category {
  kind: CategoryKind
  label: string
}

/** 分类在词条中的键名。 */
const CATEGORY_KEY = {
  tables: 'tables',
  views: 'views',
  materialized_views: 'materializedViews',
  routines: 'routines',
  sequences: 'sequences',
  synonyms: 'synonyms',
} as const satisfies Record<CategoryKind, string>

/** 分类的显示名称，按当前界面语言。 */
export function categoryLabel(kind: CategoryKind): string {
  return t(`tree.category.${CATEGORY_KEY[kind]}`)
}

/** 各类数据库在对象树中显示的分类，与 dblens-dialect 中各方言的元数据模板对应。 */
export function categoriesFor(driver: DriverId): Category[] {
  const kinds: CategoryKind[] =
    driver === 'mysql'
      ? ['tables', 'views', 'routines']
      : driver === 'postgres'
        ? ['tables', 'views', 'materialized_views', 'routines', 'sequences']
        : ['tables', 'views', 'materialized_views', 'routines', 'sequences', 'synonyms']
  return kinds.map((kind) => ({ kind, label: categoryLabel(kind) }))
}

/**
 * 节点的键。各段用 `\u0001` 连接（数据库对象名里不会出现），开头是节点类型：
 * - `c` 连接：c、profileId
 * - `s` schema：s、profileId、schema
 * - `g` 系统 schema 分组：g、profileId
 * - `k` 分类：k、profileId、schema、kind
 * - `o` 对象：o、profileId、schema、kind、name
 * - `l` 列：l、profileId、schema、table、column
 */
export type NodeKey =
  | { type: 'c'; profileId: string }
  | { type: 's'; profileId: string; schema: string }
  | { type: 'g'; profileId: string }
  | { type: 'k'; profileId: string; schema: string; kind: CategoryKind }
  | { type: 'o'; profileId: string; schema: string; kind: CategoryKind; name: string }
  | { type: 'l'; profileId: string; schema: string; table: string; column: string }

const SEP = '\u0001'

export function encodeKey(k: NodeKey): string {
  switch (k.type) {
    case 'c':
    case 'g':
      return [k.type, k.profileId].join(SEP)
    case 's':
      return [k.type, k.profileId, k.schema].join(SEP)
    case 'k':
      return [k.type, k.profileId, k.schema, k.kind].join(SEP)
    case 'o':
      return [k.type, k.profileId, k.schema, k.kind, k.name].join(SEP)
    case 'l':
      return [k.type, k.profileId, k.schema, k.table, k.column].join(SEP)
  }
}

export function decodeKey(s: string): NodeKey {
  const [type, profileId, a, b, c] = s.split(SEP)
  switch (type) {
    case 'c':
      return { type, profileId }
    case 'g':
      return { type, profileId }
    case 's':
      return { type, profileId, schema: a }
    case 'k':
      return { type, profileId, schema: a, kind: b as CategoryKind }
    case 'o':
      return { type, profileId, schema: a, kind: b as CategoryKind, name: c }
    case 'l':
      return { type, profileId, schema: a, table: b, column: c }
    default:
      throw new Error(t('tree.error.badKey', { key: s }))
  }
}

/** 过滤：名称或注释包含关键字（不区分大小写）。空关键字全部保留。 */
export function matches(filter: string, name: string, comment?: string | null): boolean {
  const f = filter.trim().toLowerCase()
  if (!f) return true
  return name.toLowerCase().includes(f) || (comment ?? '').toLowerCase().includes(f)
}

const ROUTINE_KEY = {
  procedure: 'procedure',
  function: 'function',
  package: 'package',
  package_body: 'packageBody',
} as const satisfies Record<RoutineInfo['kind'], string>

export function routineKindLabel(kind: RoutineInfo['kind']): string {
  const key = ROUTINE_KEY[kind] as (typeof ROUTINE_KEY)[RoutineInfo['kind']] | undefined
  return key ? t(`tree.routineKind.${key}`) : kind
}

/** 复制名称时用的限定名：schema.对象，列为 表.列。名字原样输出，不加引号。 */
export function qualifiedName(k: NodeKey): string {
  switch (k.type) {
    case 'o':
      return `${k.schema}.${k.name}`
    case 'l':
      return `${k.table}.${k.column}`
    case 's':
      return k.schema
    default:
      return ''
  }
}

/** child 是否在 parent 之下（不含 parent 自己）。刷新 parent 时，它下面已加载的内容都要丢掉。 */
export function isWithin(child: NodeKey, parent: NodeKey): boolean {
  if (child.profileId !== parent.profileId) return false
  switch (parent.type) {
    case 'c':
      return child.type !== 'c'
    case 'g':
      return child.type === 's'
    case 's':
      return (child.type === 'k' || child.type === 'o' || child.type === 'l') && child.schema === parent.schema
    case 'k':
      return (
        (child.type === 'o' && child.schema === parent.schema && child.kind === parent.kind) ||
        (child.type === 'l' && parent.kind === 'tables' && child.schema === parent.schema)
      )
    case 'o':
      return child.type === 'l' && child.schema === parent.schema && child.table === parent.name
    case 'l':
      return false
  }
}
