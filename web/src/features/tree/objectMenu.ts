// 对象节点的右键菜单（M1c2 设计文档第 3–5 节）：查看数据、查看 DDL、生成 SQL。这里只放纯逻辑，单独测试。

import type { DdlKind, GenerateKind, Relation, RoutineInfo, TargetKind } from '../../shared/api'
import { t } from '../../shared/i18n'
import type { CategoryKind } from './nodes'

/** 表、视图、物化视图在生成 SQL 和数据标签中的种类；其他分类返回 null。 */
export function targetKind(kind: CategoryKind): TargetKind | null {
  switch (kind) {
    case 'tables':
      return 'table'
    case 'views':
      return 'view'
    case 'materialized_views':
      return 'materialized_view'
    default:
      return null
  }
}

/** 查看 DDL 用的种类。程序按它的类型（函数、过程、包、包体）。 */
export function ddlKind(kind: CategoryKind, routineKind?: RoutineInfo['kind']): DdlKind | null {
  switch (kind) {
    case 'tables':
    case 'views':
    case 'materialized_views':
      return targetKind(kind)
    case 'sequences':
      return 'sequence'
    case 'synonyms':
      return 'synonym'
    case 'routines':
      return routineKind ?? null
  }
}

/** 菜单项：与 antd Menu 的 items 同形（只用到的字段）。 */
export interface MenuItem {
  key: string
  label: string
  disabled?: boolean
  children?: MenuItem[]
}

/** 生成 SQL 菜单项的键：`gen:select`、`gen:join`（全部外键）、`gen:join:序号`（relations 中的一项）。 */
export type GenerateChoice = { kind: GenerateKind; relation?: Relation }

/**
 * "生成 SQL"子菜单。视图只有 SELECT；表另有 INSERT、UPDATE、DELETE 和"关联查询"：
 * - relations 为 undefined 表示外键还在加载；
 * - "全部外键"只连接本表引用的表，没有时置灰；
 * - 每个外键各一项，"→ 父表"为本表引用的，"← 子表"为引用本表的。
 */
export function generateMenu(kind: TargetKind, relations: Relation[] | undefined): MenuItem[] {
  const items: MenuItem[] = [{ key: 'gen:select', label: 'SELECT' }]
  if (kind !== 'table') return items
  items.push({ key: 'gen:insert', label: 'INSERT' }, { key: 'gen:update', label: 'UPDATE' }, { key: 'gen:delete', label: 'DELETE' })
  let children: MenuItem[]
  if (!relations) children = [{ key: 'gen:loading', label: t('tree.menu.loading'), disabled: true }]
  else if (!relations.length) children = [{ key: 'gen:none', label: t('tree.menu.joinNone'), disabled: true }]
  else {
    children = [
      {
        key: 'gen:join',
        label: t('tree.menu.joinAll'),
        disabled: !relations.some((r) => r.direction === 'out'),
      },
      ...relations.map((r, i) => ({
        key: `gen:join:${i}`,
        label: `${r.direction === 'out' ? '→' : '←'} ${r.table} (${r.local_columns.join(', ')})`,
      })),
    ]
  }
  items.push({ key: 'gen:join-menu', label: t('tree.menu.join'), disabled: relations?.length === 0, children })
  return items
}

/** 由菜单项的键取回要生成的种类；不是生成 SQL 的项时返回 null。 */
export function generateChoice(key: string, relations: Relation[] | undefined): GenerateChoice | null {
  const m = /^gen:(select|insert|update|delete|join)(?::(\d+))?$/.exec(key)
  if (!m) return null
  const kind = m[1] as GenerateKind
  if (m[2] === undefined) return { kind }
  const relation = relations?.[Number(m[2])]
  return relation ? { kind, relation } : null
}
