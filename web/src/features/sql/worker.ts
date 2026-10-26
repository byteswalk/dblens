// SQL 语言 worker（M1b2 设计文档第 4 节）：解析在这里进行，不阻塞界面。只做解析，不碰元数据。

import { loadParser } from './parsers'
import type { Request, Response } from './protocol'

async function handle(r: Request): Promise<Response> {
  try {
    const p = await loadParser(r.dialect)
    switch (r.op) {
      case 'load':
        // 预热：第一次解析要建立预测缓存，较慢
        p.validate('SELECT 1', 'en_US')
        return { id: r.id, ok: true, result: null }
      case 'validate':
        return { id: r.id, ok: true, result: p.validate(r.sql, r.locale) }
      case 'suggest':
        return { id: r.id, ok: true, result: p.suggest(r.sql, r.offset) }
    }
  } catch (e) {
    return { id: r.id, ok: false, error: e instanceof Error ? e.message : String(e) }
  }
}

// tsconfig 用的是 DOM 的类型，没有 worker 全局对象的类型
const scope = self as unknown as { onmessage: (e: MessageEvent<Request>) => void; postMessage(m: Response): void }
scope.onmessage = (e) => {
  void handle(e.data).then((resp) => scope.postMessage(resp))
}
