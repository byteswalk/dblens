// 结果面板（设计稿第 2 部分）：结果集、失败的语句、消息各一个标签。每次执行后自动选中第一个结果集，
// 没有结果集时选中第一个失败的语句，再没有就选"消息"。结果集标签右侧有"导出"按钮（M1c3，执行计划的结果除外）。

import { IconAlertCircle, IconBinaryTree, IconDownload, IconMessage, IconTable } from '@tabler/icons-react'
import { Button, Spin } from 'antd'
import { type ReactNode, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { errorText } from '../../shared/api'
import type { ResultSet } from '../../shared/exec'
import type { TabRuntime } from '../editor/runner'
import type { Order } from '../data/sort'
import ResultGrid from './ResultGrid'
import { type ResultTab, messageLines, resultTabs } from './results'

export interface ResultsPanelProps {
  rt: TabRuntime
  onFetchMore(): void
  /** 数据标签的排序（见 ResultGrid）。 */
  sort?: Order
  onSort?(column: string): void
  /** 导出第 `stmt` 条语句的结果；不给时不显示导出按钮。 */
  onExport?(stmt: number): void
  /** 自己画结果集（数据标签的可编辑表格，M2a）；不给时用只读的结果表格。 */
  renderResult?(result: ResultSet, waiting: boolean): ReactNode
}

function TabIcon({ tab, explain }: { tab: ResultTab; explain: boolean }) {
  if (tab.kind === 'error') return <IconAlertCircle size={16} />
  if (tab.kind === 'messages') return <IconMessage size={16} />
  return explain ? <IconBinaryTree size={16} /> : <IconTable size={16} />
}

export default function ResultsPanel({ rt, onFetchMore, sort, onSort, onExport, renderResult }: ResultsPanelProps) {
  const { t } = useTranslation()
  const { exec, explain, capped, running } = rt
  const tabs = resultTabs(exec, explain, capped)
  // null 表示自动：跟着执行进度选中第一个结果；用户点过标签后保持用户的选择，直到下一次执行
  const [selected, setSelected] = useState<string | null>(null)
  useEffect(() => setSelected(null), [rt.submission])
  const first = tabs.find((x) => x.kind === 'result') ?? tabs.find((x) => x.kind === 'error') ?? tabs.at(-1)!
  const current = tabs.find((x) => x.key === selected) ?? first

  const lastStmt = exec.statements.length - 1
  return (
    <div className="results">
      <div className="row rtabs">
        {tabs.map((tab) => (
          <span
            key={tab.key}
            className={`rtab${tab.key === current.key ? ' on' : ''}${tab.kind === 'error' ? ' bad' : ''}`}
            onClick={() => setSelected(tab.key)}
          >
            <TabIcon tab={tab} explain={explain} />
            {tab.label}
            {tab.kind === 'result' && (
              <span className="muted">
                {t(tab.capped ? 'results.tab.rowsCapped' : 'results.tab.rows', { count: tab.rows })}
              </span>
            )}
          </span>
        ))}
        <span className="grow" />
        {running && <Spin size="small" style={{ marginRight: 8 }} />}
        {onExport && current.kind === 'result' && !explain && (
          <Button
            type="text"
            size="small"
            icon={<IconDownload size={16} />}
            onClick={() => onExport(current.stmt)}
            style={{ marginRight: 4 }}
          >
            {t('results.export.button')}
          </Button>
        )}
      </div>
      <div className="results-body">
        {current.kind === 'result' &&
          renderResult?.(
            exec.statements[current.stmt].results[current.result],
            exec.waitingForMore && current.stmt === lastStmt,
          )}
        {current.kind === 'result' && !renderResult && (
          <ResultGrid
            key={current.key}
            result={exec.statements[current.stmt].results[current.result]}
            waiting={exec.waitingForMore && current.stmt === lastStmt}
            onFetchMore={onFetchMore}
            sort={sort}
            onSort={onSort}
          />
        )}
        {current.kind === 'error' && (
          <div className="result-error">
            <div className="err">{errorText(exec.statements[current.stmt].error!)}</div>
            <pre>{rt.source.slice(exec.statements[current.stmt].start, exec.statements[current.stmt].end)}</pre>
          </div>
        )}
        {current.kind === 'messages' && (
          <div className="messages">
            {rt.error && <div className="err">{rt.error}</div>}
            {messageLines(exec, capped, rt.declined).map((m, i) => (
              <div key={i} className={m.level === 'error' ? 'err' : m.level === 'warn' ? 'warn' : undefined}>
                {m.text}
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
