// 全局唯一的 SQL 语言服务客户端。worker 在第一次使用时才创建。

import { SqlClient } from './client'
import SqlWorker from './worker?worker'

export const sqlClient = new SqlClient(() => new SqlWorker())
