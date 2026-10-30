// 编辑器工具栏（设计稿第 2 部分）：执行当前语句、执行全部、停止、出错停止 / 继续；连接和 schema；
// 事务模式、提交、回滚；收藏、格式化、执行计划。会话级操作出错时用全局提示显示。

import { App, Button, Select, Tooltip } from 'antd'
import {
  IconAlertTriangle,
  IconArrowBackUp,
  IconBinaryTree,
  IconCheck,
  IconPlayerPlay,
  IconPlayerStop,
  IconPlayerTrackNext,
  IconStar,
  IconWand,
} from '@tabler/icons-react'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import EnvTag from '../../shared/EnvTag'
import type { Profile } from '../../shared/api'
import { sendCommand } from './commands'
import { type Runner, type TabRuntime, disposeRunner } from './runner'
import { type QueryTab, useTabs } from './tabs'

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
  /** 自动刷新的间隔（秒），0 表示关闭。 */
  refresh: number
  onRefreshChange(seconds: number): void
}

export default function Toolbar({ tab, rt, runner, profiles, refresh, onRefreshChange }: ToolbarProps) {
  const api = useApi()
  const { t } = useTranslation()
  const { message, modal } = App.useApp()
  const driver = profiles.find((p) => p.id === tab.profileId)?.driver
  // SQL Server 的会话按库切换（USE），下拉框列出库
  const { data: schemas } = useQuery({
    queryKey: ['schema-list', tab.profileId, driver],
    queryFn: () => (driver === 'sqlserver' ? api.catalogs(tab.profileId) : api.schemas(tab.profileId)),
  })
  const guard = (p: Promise<unknown>) => p.catch((e: unknown) => message.error(e instanceof Error ? e.message : String(e)))
  const s = rt.session

  const switchProfile = (profileId: string) => {
    const go = () => disposeRunner(tab.id).then(() => useTabs.getState().setProfile(tab.id, profileId))
    if (s?.in_transaction) {
      modal.confirm({ title: t('editor.switchProfile.title'), content: t('editor.switchProfile.content'), onOk: go })
    } else void go()
  }

  return (
    <div className="row toolbar">
      <IconBtn title={t('editor.toolbar.runCurrent')} color="var(--ok)" disabled={rt.running} onClick={() => sendCommand('run-current')}>
        <IconPlayerPlay size={16} />
      </IconBtn>
      <IconBtn title={t('editor.toolbar.runAll')} disabled={rt.running} onClick={() => sendCommand('run-all')}>
        <IconPlayerTrackNext size={16} />
      </IconBtn>
      <IconBtn title={t('editor.toolbar.stop')} color="var(--danger)" disabled={!rt.running} onClick={() => runner.cancel()}>
        <IconPlayerStop size={16} />
      </IconBtn>
      <Select
        size="small"
        variant="filled"
        value={rt.continueOnError ? 'continue' : 'stop'}
        onChange={(v) => runner.setContinueOnError(v === 'continue')}
        options={[
          { value: 'stop', label: t('editor.toolbar.stopOnError') },
          { value: 'continue', label: t('editor.toolbar.continueOnError') },
        ]}
        style={{ width: 92 }}
      />
      <Select
        size="small"
        variant="filled"
        value={refresh}
        onChange={onRefreshChange}
        options={[0, 5, 10, 30, 60].map((s) => ({
          value: s,
          label: s ? t('editor.toolbar.refreshEvery', { seconds: s }) : t('editor.toolbar.refreshOff'),
        }))}
        popupMatchSelectWidth={false}
        style={{ width: 118 }}
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
              {p.name}
              <EnvTag env={p.environment} />
            </span>
          ),
        }))}
        style={{ minWidth: 140 }}
      />
      <Select
        size="small"
        variant="filled"
        showSearch
        placeholder={driver === 'sqlserver' ? t('editor.toolbar.database') : 'schema'}
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
          { value: 'auto', label: t('editor.toolbar.autoCommit') },
          { value: 'manual', label: t('editor.toolbar.manualCommit') },
        ]}
        style={{ width: 92 }}
      />
      {s?.autocommit === false && (
        <>
          <Button size="small" icon={<IconCheck size={14} />} disabled={rt.running} onClick={() => guard(runner.commit())}>
            {t('editor.toolbar.commit')}
          </Button>
          <Button size="small" icon={<IconArrowBackUp size={14} />} disabled={rt.running} onClick={() => guard(runner.rollback())}>
            {t('editor.toolbar.rollback')}
          </Button>
        </>
      )}
      {s?.in_transaction && (
        <span className="row" style={{ color: 'var(--warn)', gap: 4 }}>
          <IconAlertTriangle size={14} />
          {t('editor.toolbar.inTransaction')}
        </span>
      )}
      <span className="grow" />
      <IconBtn title={t('editor.toolbar.favorite')} onClick={() => sendCommand('favorite')}>
        <IconStar size={16} />
      </IconBtn>
      <IconBtn title={t('editor.toolbar.format')} onClick={() => sendCommand('format')}>
        <IconWand size={16} />
      </IconBtn>
      <IconBtn title={t('editor.toolbar.explain')} disabled={rt.running} onClick={() => sendCommand('explain')}>
        <IconBinaryTree size={16} />
      </IconBtn>
    </div>
  )
}
