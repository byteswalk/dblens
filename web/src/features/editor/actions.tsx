// 新建、关闭查询标签页。菜单、快捷键、对象树、标签栏共用；不引入 Monaco，外壳可以直接使用。

import { App, Button } from 'antd'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { useUi } from '../../app/ui'
import { hasPendingEdits, useEdits } from '../data/editStore'
import { sendInsert } from './commands'
import { disposeRunner, useRuntime } from './runner'
import { markRun, useTabs } from './tabs'

export function useQueryActions() {
  const api = useApi()
  const { t } = useTranslation()
  const { modal } = App.useApp()
  const { data: profiles } = useQuery({ queryKey: ['profiles'], queryFn: () => api.profiles() })

  /** 新建查询：用给定的连接，否则依次用对象树中选中的、当前标签的、第一个连接；一个连接都没有时打开新建连接。 */
  const newQuery = (profileId?: string) => {
    const { tabs, active } = useTabs.getState()
    const id =
      profileId ??
      useUi.getState().selectedProfile ??
      tabs.find((x) => x.id === active)?.profileId ??
      profiles?.[0]?.id
    if (id) useTabs.getState().open(id)
    else useUi.getState().openConnection(null)
  }

  /** 用给定的 SQL 新开标签：连接已删除或没给时，依次用对象树中选中的、当前标签的、第一个连接。`run` 为 true 时打开后立即执行。 */
  const openSql = (sql: string, profileId?: string, { run = false } = {}) => {
    const { tabs, active } = useTabs.getState()
    const exists = (id?: string | null) => (id && profiles?.some((p) => p.id === id) ? id : undefined)
    const id =
      exists(profileId) ??
      exists(useUi.getState().selectedProfile) ??
      exists(tabs.find((x) => x.id === active)?.profileId) ??
      profiles?.[0]?.id
    if (id) {
      const tabId = useTabs.getState().open(id, sql)
      if (run) markRun(tabId)
    } else useUi.getState().openConnection(null)
  }

  /** 插入到当前编辑器的光标处；当前不是查询标签（没有标签或是数据标签）时新开一个。 */
  const insertSql = (sql: string, profileId?: string) => {
    const { tabs, active } = useTabs.getState()
    const tab = tabs.find((x) => x.id === active)
    if (tab && !tab.data) sendInsert(sql)
    else openSql(sql, profileId)
  }

  /** 关闭标签页：数据标签有未提交的编辑时先确认放弃；有未提交的事务时先问提交还是回滚。对话框结束（关闭或取消）后完成。 */
  const closeQuery = (tabId: string): Promise<void> =>
    new Promise((done) => {
      const tab = useTabs.getState().tabs.find((x) => x.id === tabId)
      if (!tab) return done()
      if (tab.data && hasPendingEdits(tabId)) {
        modal.confirm({
          title: t('editor.data.edit.discardTitle'),
          content: t('editor.data.edit.discardContent'),
          okText: t('editor.data.edit.discardOk'),
          okType: 'danger',
          onOk: () => {
            useEdits.getState().clear(tabId)
            void closeQuery(tabId).then(done)
          },
          onCancel: () => done(),
        })
        return
      }
      const finish = async (commit: boolean) => {
        await disposeRunner(tabId, commit)
        useEdits.getState().clear(tabId)
        useTabs.getState().close(tabId)
        done()
      }
      if (!useRuntime.getState().tabs[tabId]?.session?.in_transaction) {
        void finish(false)
        return
      }
      const m = modal.confirm({
        title: t('editor.closeTab.title', { title: tab.title }),
        content: t('editor.closeTab.content'),
        okText: t('editor.closeTab.commit'),
        cancelText: t('editor.closeTab.cancel'),
        onOk: () => finish(true),
        onCancel: () => done(),
        footer: (_, { OkBtn, CancelBtn }) => (
          <>
            <CancelBtn />
            <Button
              danger
              onClick={() => {
                m.destroy()
                void finish(false)
              }}
            >
              {t('editor.closeTab.rollback')}
            </Button>
            <OkBtn />
          </>
        ),
      })
    })

  /** 依次关闭多个标签；要确认的标签先切过去再问，取消的留下，接着处理下一个。 */
  const closeMany = async (ids: string[]) => {
    for (const id of ids) {
      const needsAsk = hasPendingEdits(id) || useRuntime.getState().tabs[id]?.session?.in_transaction
      if (needsAsk) useTabs.getState().activate(id)
      await closeQuery(id)
    }
  }

  const closeActive = () => {
    const active = useTabs.getState().active
    if (active) void closeQuery(active)
  }

  return { newQuery, openSql, insertSql, closeQuery, closeMany, closeActive }
}
