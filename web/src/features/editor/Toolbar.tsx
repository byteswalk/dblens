// 编辑器工具栏（设计稿第 2 部分）：执行当前语句、执行全部、停止、出错停止 / 继续；连接和 schema；
// 事务模式、提交、回滚；格式化、执行计划。会话级操作出错时用全局提示显示。

import { App, Button, Select, Tooltip } from 'antd'
import {
  IconAlertTriangle,
  IconArrowBackUp,
  IconBinaryTree,
  IconCheck,
  IconPlayerPlay,
  IconPlayerStop,
  IconPlayerTrackNext,
  IconWand,
} from '@tabler/icons-react'
import { useQuery } from '@tanstack/react-query'

import { useApi } from '../../app/context'
import type { Profile } from '../../shared/api'
import { sendCommand } from './commands'
import { type Runner, type TabRuntime, disposeRunner } from './runner'
import { type QueryTab, useTabs } from './tabs'

const ENV_COLORS: Record<string, string> = {
  dev: 'var(--env-dev)',
  test: 'var(--env-test)',
  prod: 'var(--env-prod)',
  none: 'var(--env-none)',
}

function IconBtn({ title, onClick, disabled, color, children }: {
  title: string
  onClick(): void
  disabled?: boolean
  color?: string
  children: React.ReactNode
}) {
  return (
    <Tooltip title={title} mouseEnterDelay={0.6}>
      <Button type="text" size="small" disabled={disabled} onClick={onClick} style={{ color: disabled ? undefined : color }}>
        {children}
      </Button>
    </Tooltip>
  )
}

export interface ToolbarProps {
  tab: QueryTab
  rt: TabRuntime
  runner: Runner
  profiles: Profile[]
}

export default function Toolbar({ tab, rt, runner, profiles }: ToolbarProps) {
  const api = useApi()
  const { message, modal } = App.useApp()
  const { data: schemas } = useQuery({
    queryKey: ['schema-list', tab.profileId],
    queryFn: () => api.schemas(tab.profileId),
  })
  const guard = (p: Promise<unknown>) => p.catch((e: unknown) => message.error(e instanceof Error ? e.message : String(e)))
  const s = rt.session

  const switchProfile = (profileId: string) => {
    const go = () => disposeRunner(tab.id).then(() => useTabs.getState().setProfile(tab.id, profileId))
    if (s?.in_transaction) {
      modal.confirm({ title: '切换连接', content: '当前会话有未提交的事务，切换连接会回滚这些修改。', onOk: go })
    } else void go()
  }

  return (
    <div className="row toolbar">
      <IconBtn title="执行当前语句（Ctrl+Enter）" color="var(--ok)" disabled={rt.running} onClick={() => sendCommand('run-current')}>
        <IconPlayerPlay size={16} />
      </IconBtn>
      <IconBtn title="执行全部（F5）" disabled={rt.running} onClick={() => sendCommand('run-all')}>
        <IconPlayerTrackNext size={16} />
      </IconBtn>
      <IconBtn title="停止（Ctrl+Break）" color="var(--danger)" disabled={!rt.running} onClick={() => runner.cancel()}>
        <IconPlayerStop size={16} />
      </IconBtn>
      <Select
        size="small"
        variant="filled"
        value={rt.continueOnError ? 'continue' : 'stop'}
        onChange={(v) => runner.setContinueOnError(v === 'continue')}
        options={[
          { value: 'stop', label: '出错停止' },
          { value: 'continue', label: '出错继续' },
        ]}
        style={{ width: 92 }}
      />
      <span className="sep" />
      <Select
        size="small"
        variant="filled"
        value={tab.profileId}
        onChange={switchProfile}
        disabled={rt.running}
        popupMatchSelectWidth={false}
        options={profiles.map((p) => ({
          value: p.id,
          label: (
            <span className="row" style={{ gap: 6 }}>
              <span className="dot" style={{ background: ENV_COLORS[p.environment ?? 'none'] }} />
              {p.name}
            </span>
          ),
        }))}
        style={{ minWidth: 140 }}
      />
      <Select
        size="small"
        variant="filled"
        showSearch
        placeholder="schema"
        value={s?.schema ?? undefined}
        onChange={(v: string) => guard(runner.setSchema(v))}
        disabled={rt.running}
        popupMatchSelectWidth={false}
        options={(schemas ?? []).map((x) => ({ value: x.name, label: x.name }))}
        style={{ minWidth: 120 }}
      />
      <span className="sep" />
      <Select
        size="small"
        variant="filled"
        value={s?.autocommit === false ? 'manual' : 'auto'}
        onChange={(v) => guard(runner.setAutocommit(v === 'auto'))}
        disabled={rt.running}
        options={[
          { value: 'auto', label: '自动提交' },
          { value: 'manual', label: '手动提交' },
        ]}
        style={{ width: 92 }}
      />
      {s?.autocommit === false && (
        <>
          <Button size="small" icon={<IconCheck size={14} />} disabled={rt.running} onClick={() => guard(runner.commit())}>
            提交
          </Button>
          <Button size="small" icon={<IconArrowBackUp size={14} />} disabled={rt.running} onClick={() => guard(runner.rollback())}>
            回滚
          </Button>
        </>
      )}
      {s?.in_transaction && (
        <span className="row" style={{ color: 'var(--warn)', gap: 4 }}>
          <IconAlertTriangle size={14} />
          有未提交的事务
        </span>
      )}
      <span className="grow" />
      <IconBtn title="格式化（Shift+Alt+F）" onClick={() => sendCommand('format')}>
        <IconWand size={16} />
      </IconBtn>
      <IconBtn title="执行计划" disabled={rt.running} onClick={() => sendCommand('explain')}>
        <IconBinaryTree size={16} />
      </IconBtn>
    </div>
  )
}
