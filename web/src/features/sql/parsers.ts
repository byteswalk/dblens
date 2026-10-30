// 按语法族懒加载解析器：每种是一个单独的块，用到时才下载（MySQL、PG 各约 0.35 MB，PL/SQL 约 1.4 MB，gzip 后；
// T-SQL 与 PL/SQL 相近）。

import type { Dialect, SqlParser } from './types'

const loaded = new Map<Dialect, Promise<SqlParser>>()

export function loadParser(d: Dialect): Promise<SqlParser> {
  let p = loaded.get(d)
  if (!p) {
    switch (d) {
      case 'mysql':
        p = import('./mysql').then((m) => m.mysql)
        break
      case 'postgres':
        p = import('./postgres').then((m) => m.postgres)
        break
      case 'plsql':
        p = import('./plsql/parser').then((m) => m.plsql)
        break
      case 'tsql':
        p = import('./tsql/parser').then((m) => m.tsql)
        break
    }
    // 加载失败时不缓存，下次重试
    p.catch(() => loaded.delete(d))
    loaded.set(d, p)
  }
  return p
}
