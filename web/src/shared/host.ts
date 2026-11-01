// 工作台嵌在别的页面里时，宿主提供的扩展点。单机版没有宿主，全部取默认值。

import { createContext, useContext } from 'react'

export interface TicketRequest {
  kind: 'sql' | 'export'
  profileId: string
  schema: string | null
  sql: string
  reason: string
}

export interface Host {
  /** 这个入口要隐藏吗。key 是菜单项的 key，树的右键菜单加 `tree:` 前缀。 */
  hidden?: (key: string) => boolean
  /** 语句要走工单时调用。 */
  onTicket?: (request: TicketRequest) => void
}

export const HostContext = createContext<Host>({})

export function useHost(): Host {
  return useContext(HostContext)
}

/** 按 key 过滤后，去掉开头、结尾和连续的分隔项。 */
export function withoutHidden<T>(items: T[], visible: (item: T) => boolean, divider: (item: T) => boolean): T[] {
  const out: T[] = []
  for (const item of items) {
    if (divider(item)) {
      if (out.length && !divider(out[out.length - 1])) out.push(item)
    } else if (visible(item)) out.push(item)
  }
  while (out.length && divider(out[out.length - 1])) out.pop()
  return out
}
