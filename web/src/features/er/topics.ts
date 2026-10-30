// 业务主题：按表名、表注释自动归类，用户可以改名、新建、删除，把表移到别的主题。
// 改动存在界面设置里，按连接（或模型）和 schema 区分。

import type { ErTable } from '../../shared/api'

/** 用户的改动。`names` 是主题编号 → 显示名，`tables` 是表名 → 主题编号。 */
export interface TopicEdits {
  names: Record<string, string>
  tables: Record<string, string>
}

export const EMPTY_EDITS: TopicEdits = { names: {}, tables: {} }

export const OTHER_TOPIC = '__other'

export interface Topic {
  id: string
  name: string
  tables: string[]
}

/** 常见的表名分段对应的中文。 */
const WORDS: Record<string, string> = {
  AUTH: '认证授权',
  BASE: '基础数据',
  BIZ: '业务',
  CFG: '配置',
  CONFIG: '配置',
  CUST: '客户',
  CUSTOMER: '客户',
  DICT: '数据字典',
  DOC: '文档',
  FILE: '文件',
  FIN: '财务',
  HR: '人事',
  INV: '库存',
  JOB: '任务调度',
  LOG: '日志',
  MENU: '菜单',
  MODEL: '模型',
  MSG: '消息',
  ORDER: '订单',
  ORG: '组织',
  PAY: '支付',
  PROD: '产品',
  PRODUCT: '产品',
  PUB: '公共',
  RPT: '报表',
  REPORT: '报表',
  ROLE: '角色',
  STAT: '统计',
  SYS: '系统管理',
  SYSTEM: '系统管理',
  TASK: '任务',
  USER: '用户',
  WF: '工作流',
}

/** 几条注释共同的开头，太短时返回空串。 */
function commonPrefix(comments: string[]): string {
  if (comments.length < 2) return ''
  let p = comments[0]
  for (const c of comments) {
    while (p && !c.startsWith(p)) p = p.slice(0, -1)
  }
  p = p.replace(/[\s:：,，、(（-]+$/, '')
  return p.length >= 2 ? p : ''
}

/** 自动归类：大多数表共用的第一段是系统前缀，取下一段；只有一张表的主题和分不出的表归入其他。 */
export function autoTopics(tables: ErTable[]): Map<string, { code: string; name: string; tables: string[] }> {
  const names = tables.map((t) => t.table.name)
  const parts = names.map((n) => n.toUpperCase().split('_').filter(Boolean))
  const firsts = new Map<string, number>()
  for (const p of parts) firsts.set(p[0] ?? '', (firsts.get(p[0] ?? '') ?? 0) + 1)
  const common = Math.max(0, ...firsts.values())
  const index = names.length >= 4 && common / names.length >= 0.6 ? 1 : 0
  const keyOf = (p: string[]) => (p.length > index + 1 ? p[index] : OTHER_TOPIC)
  const groups = new Map<string, string[]>()
  names.forEach((n, i) => {
    const key = keyOf(parts[i])
    groups.set(key, [...(groups.get(key) ?? []), n])
  })
  const result = new Map<string, { code: string; name: string; tables: string[] }>()
  const other: string[] = []
  for (const [code, list] of groups) {
    if (code === OTHER_TOPIC || list.length < 2) {
      other.push(...list)
      continue
    }
    const comments = list.flatMap((n) => {
      const c = tables.find((t) => t.table.name === n)?.table.comment
      return c ? [c] : []
    })
    const name = WORDS[code] ?? (comments.length === list.length ? commonPrefix(comments) : '') ?? ''
    result.set(code, { code, name: name || code, tables: list })
  }
  if (other.length) result.set(OTHER_TOPIC, { code: OTHER_TOPIC, name: '', tables: other })
  return result
}

/** 自动结果叠加用户的改动，得到主题列表（表多的在前，其他在最后）。 */
export function resolveTopics(tables: ErTable[], edits: TopicEdits, otherName: string): Topic[] {
  const auto = autoTopics(tables)
  const topics = new Map<string, Topic>()
  for (const [id, a] of auto) topics.set(id, { id, name: edits.names[id] ?? (a.name || otherName), tables: [] })
  for (const [id, name] of Object.entries(edits.names)) {
    if (!topics.has(id)) topics.set(id, { id, name, tables: [] })
  }
  if (!topics.has(OTHER_TOPIC)) topics.set(OTHER_TOPIC, { id: OTHER_TOPIC, name: edits.names[OTHER_TOPIC] ?? otherName, tables: [] })
  const autoOf = new Map<string, string>()
  for (const [id, a] of auto) for (const n of a.tables) autoOf.set(n, id)
  for (const t of tables) {
    const n = t.table.name
    const id = edits.tables[n] ?? autoOf.get(n) ?? OTHER_TOPIC
    ;(topics.get(id) ?? topics.get(OTHER_TOPIC)!).tables.push(n)
  }
  const list = [...topics.values()].filter((x) => x.tables.length || edits.names[x.id] !== undefined || x.id === OTHER_TOPIC)
  return list.sort((a, b) => {
    if (a.id === OTHER_TOPIC || b.id === OTHER_TOPIC) return a.id === OTHER_TOPIC ? 1 : -1
    return b.tables.length - a.tables.length
  })
}

/** 表名 → 主题显示名。 */
export function topicOfTable(topics: Topic[]): Map<string, string> {
  return new Map(topics.flatMap((x) => x.tables.map((n) => [n, x.name] as const)))
}

/** 设置项名称有长度限制，连接（或模型路径）和 schema 取哈希。 */
export function topicsKey(scope: string): string {
  let h = 5381
  for (const c of scope) h = (Math.imul(h, 33) ^ (c.codePointAt(0) ?? 0)) >>> 0
  return `ui.er.topics.${h.toString(36)}`
}

export function parseEdits(raw: string | undefined): TopicEdits {
  if (!raw) return EMPTY_EDITS
  try {
    const v = JSON.parse(raw) as Partial<TopicEdits>
    return { names: v.names ?? {}, tables: v.tables ?? {} }
  } catch {
    return EMPTY_EDITS
  }
}

export function moveTables(edits: TopicEdits, tables: string[], topic: string): TopicEdits {
  return { ...edits, tables: { ...edits.tables, ...Object.fromEntries(tables.map((n) => [n, topic])) } }
}

/** 删除主题：它现在的表都归入其他，否则自动分出的主题下次又会冒出来。 */
export function deleteTopic(edits: TopicEdits, topic: Topic): TopicEdits {
  const names = { ...edits.names }
  delete names[topic.id]
  return { names, tables: moveTables(edits, topic.tables, OTHER_TOPIC).tables }
}

/** 设定主题包含的表：新选的移入，去掉的归入其他。记下名字，表都移走时主题还在。 */
export function setTopicTables(edits: TopicEdits, topic: Topic, tables: string[]): TopicEdits {
  const removed = topic.tables.filter((n) => !tables.includes(n))
  return {
    names: { ...edits.names, [topic.id]: topic.name },
    tables: moveTables(moveTables(edits, removed, OTHER_TOPIC), tables, topic.id).tables,
  }
}
