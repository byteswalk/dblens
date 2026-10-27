// 数据标签的排序（M1c2 设计文档第 5 节）：点列头依次切换升序、降序、不排序；同一时间只按一列排序。

export type Order = { column: string; desc: boolean } | null

/** 点了 column 的列头之后的排序。 */
export function nextOrder(current: Order | undefined, column: string): Order {
  if (!current || current.column !== column) return { column, desc: false }
  return current.desc ? null : { column, desc: true }
}

/** 列头的标题：排序的列后面加箭头。 */
export function sortedTitle(name: string, order: Order | undefined): string {
  if (order?.column !== name) return name
  return `${name} ${order.desc ? '↓' : '↑'}`
}
