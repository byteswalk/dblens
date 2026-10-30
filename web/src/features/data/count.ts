// 数据标签的"统计行数"：另开一个会话执行 COUNT(*)，取第一行第一列后关闭。
// 不用标签自己的会话：它可能还有没读完的结果（服务端在等续取），会话正忙。

import { type Api, errorText, splitCatalog } from '../../shared/api'
import { ExecSocket, cellText } from '../../shared/exec'
import { t } from '../../shared/i18n'
import { withVault } from '../connections/vault'

/** `schema`：对象所在的 schema；SQL Server 的合成 schema 先把会话切到其中的库。 */
export async function countRows(api: Api, profileId: string, sql: string, schema?: string): Promise<string> {
  const session = await withVault(api, () => api.openSession(profileId))
  const catalog = schema === undefined ? undefined : splitCatalog(schema).catalog
  if (catalog !== undefined) await api.setSchema(session.id, schema!)
  let socket: ExecSocket | null = null
  try {
    return await new Promise<string>((resolve, reject) => {
      let value: string | null = null
      socket = new ExecSocket(
        api.wsUrl(session.id),
        (ev) => {
          if (ev.type === 'rows' && value === null && ev.rows.length) value = cellText(ev.rows[0][0])
          else if (ev.type === 'failed' || ev.type === 'error') reject(new Error(errorText(ev.error)))
          else if (ev.type === 'done') resolve(value ?? '')
        },
        () => reject(new Error(t('editor.runner.serviceDisconnected'))),
      )
      socket.exec(sql).catch(reject)
    })
  } finally {
    ;(socket as ExecSocket | null)?.close()
    await api.closeSession(session.id).catch(() => {})
  }
}
