// 新建、关闭查询标签页。菜单、快捷键、对象树、标签栏共用；不引入 Monaco，外壳可以直接使用。

import { App, Button } from 'antd'
import { useQuery } from '@tanstack/react-query'

import { useApi } from '../../app/context'
import { useUi } from '../../app/ui'
import { disposeRunner, useRuntime } from './runner'
import { useTabs } from './tabs'

export function useQueryActions() {
  const api = useApi()
  const { modal } = App.useApp()
  const { data: profiles } = useQuery({ queryKey: ['profiles'], queryFn: () => api.profiles() })

  /** 新建查询：用给定的连接，否则依次用对象树中选中的、当前标签的、第一个连接；一个连接都没有时打开新建连接。 */
  const newQuery = (profileId?: string) => {
    const { tabs, active } = useTabs.getState()
    const id =
      profileId ??
      useUi.getState().selectedProfile ??
      tabs.find((t) => t.id === active)?.profileId ??
      profiles?.[0]?.id
    if (id) useTabs.getState().open(id)
    else useUi.getState().openConnection(null)
  }

  /** 关闭标签页：有未提交的事务时先问提交还是回滚。 */
  const closeQuery = (tabId: string) => {
    const tab = useTabs.getState().tabs.find((t) => t.id === tabId)
    if (!tab) return
    const finish = async (commit: boolean) => {
      await disposeRunner(tabId, commit)
      useTabs.getState().close(tabId)
    }
    if (!useRuntime.getState().tabs[tabId]?.session?.in_transaction) {
      void finish(false)
      return
    }
    const m = modal.confirm({
      title: `${tab.title} 有未提交的事务`,
      content: '关闭前提交还是回滚这些修改？',
      okText: '提交并关闭',
      cancelText: '取消',
      onOk: () => finish(true),
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
            回滚并关闭
          </Button>
          <OkBtn />
        </>
      ),
    })
  }

  const closeActive = () => {
    const active = useTabs.getState().active
    if (active) closeQuery(active)
  }

  return { newQuery, closeQuery, closeActive }
}
