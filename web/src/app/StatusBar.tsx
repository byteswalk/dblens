// 状态栏：对象树中选中的连接。执行相关的信息（事务、行数、耗时、光标位置）在 M1b 加入。

import { IconPlugConnected } from '@tabler/icons-react'
import { useQuery } from '@tanstack/react-query'

import { DRIVERS } from '../features/connections/form'
import { useApi } from './context'
import { useUi } from './ui'

export default function StatusBar() {
  const api = useApi()
  const selected = useUi((s) => s.selectedProfile)
  const { data: profiles } = useQuery({ queryKey: ['profiles'], queryFn: () => api.profiles() })
  const p = profiles?.find((x) => x.id === selected)
  return (
    <div className="row statusbar">
      {p ? (
        <span className="row" style={{ gap: 4 }}>
          <IconPlugConnected size={14} />
          {p.name} · {DRIVERS.find((d) => d.id === p.driver)?.label} · {p.user}@{p.host}:{p.port}
        </span>
      ) : (
        <span className="muted">未选择连接</span>
      )}
      <span className="grow" />
    </div>
  )
}
