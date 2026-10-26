// 收藏的纯逻辑（M1c1 设计文档第 6 节）：平铺列表组成树、移动是否合法、拖放后的目标位置、默认名称。

import type { Favorite } from '../../shared/api'

export interface FavNode {
  fav: Favorite
  children: FavNode[]
}

/** 平铺列表 → 树。同一文件夹内按 position 排序；上级不存在的当作根。 */
export function buildTree(favs: Favorite[]): FavNode[] {
  const nodes = new Map(favs.map((f) => [f.id, { fav: f, children: [] as FavNode[] }]))
  const roots: FavNode[] = []
  for (const n of nodes.values()) {
    const parent = n.fav.parent_id ? nodes.get(n.fav.parent_id) : undefined
    ;(parent ? parent.children : roots).push(n)
  }
  const sort = (list: FavNode[]) => {
    list.sort((a, b) => a.fav.position - b.fav.position || a.fav.created_at - b.fav.created_at)
    for (const n of list) sort(n.children)
  }
  sort(roots)
  return roots
}

/** 能否把 `id` 移到文件夹 `parent`（null 为根）中：目标必须是文件夹，且不是它自己或它的下级。 */
export function canMoveInto(favs: Favorite[], id: string, parent: string | null): boolean {
  const byId = new Map(favs.map((f) => [f.id, f]))
  if (parent !== null && byId.get(parent)?.kind !== 'folder') return false
  for (let p: string | null = parent; p !== null; p = byId.get(p)?.parent_id ?? null) {
    if (p === id) return false
  }
  return true
}

/**
 * 拖放的目标：拖到 `target` 之前（-1）、之后（1）或里面（0，只对文件夹）。
 * 返回移动请求（位置是在去掉被拖动项后的同级中的下标）；不合法时为 null。
 */
export function dropTarget(
  favs: Favorite[],
  dragId: string,
  targetId: string,
  where: -1 | 0 | 1,
): { parent_id: string | null; position: number } | null {
  const target = favs.find((f) => f.id === targetId)
  if (!target || dragId === targetId) return null
  if (where === 0) {
    if (target.kind !== 'folder' || !canMoveInto(favs, dragId, target.id)) return null
    return { parent_id: target.id, position: 0 }
  }
  const parent = target.parent_id
  if (!canMoveInto(favs, dragId, parent)) return null
  const siblings = favs
    .filter((f) => f.parent_id === parent && f.id !== dragId)
    .sort((a, b) => a.position - b.position)
  const index = siblings.findIndex((f) => f.id === targetId)
  return { parent_id: parent, position: index + (where === 1 ? 1 : 0) }
}

/** 收藏的默认名称：SQL 开头的内容，换行和连续空白合成一个空格，最长 60 个字符。 */
export function defaultName(sql: string): string {
  const text = sql.trim().replace(/\s+/g, ' ')
  return text.length > 60 ? `${text.slice(0, 59)}…` : text
}

/** 文件夹的完整路径（根到它），用于文件夹选择框中显示。 */
export function folderPath(favs: Favorite[], id: string): string[] {
  const byId = new Map(favs.map((f) => [f.id, f]))
  const out: string[] = []
  for (let f = byId.get(id); f; f = f.parent_id ? byId.get(f.parent_id) : undefined) out.unshift(f.name)
  return out
}
