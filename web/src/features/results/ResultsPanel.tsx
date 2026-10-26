// 结果面板（设计稿第 2 部分）：结果集、失败的语句、消息各一个标签。每次执行后自动选中第一个结果集，
// 没有结果集时选中第一个失败的语句，再没有就选"消息"。

import { IconAlertCircle, IconBinaryTree, IconMessage, IconTable } from '@tabler/icons-react'
import { Spin } from 'antd'
import { useEffect, useState } from 'react'

import { errorText } from '../../shared/api'
import type { TabRuntime } from '../editor/runner'
import ResultGrid from './ResultGrid'
import { type ResultTab, messageLines, resultTabs } from './results'

export interface ResultsPanelProps {
  rt: TabRuntime
  onFetchMore(): void
}

function TabIcon({ t, explain }: { t: ResultTab; explain: boolean }) {
  if (t.kind === 'error') return <IconAlertCircle size={14} />
  if (t.kind === 'messages') return <IconMessage size={14} />
  return explain ? <IconBinaryTree size={14} /> : <IconTable size={14} />
}

export default function ResultsPanel({ rt, onFetchMore }: ResultsPanelProps) {
  const { exec, explain, capped, running } = rt
  const tabs = resultTabs(exec, explain, capped)
  // null 表示自动：跟着执行进度选中第一个结果；用户点过标签后保持用户的选择，直到下一次执行
  const [selected, setSelected] = useState<string | null>(null)
  useEffect(() => setSelected(null), [rt.submission])
  const first = tabs.find((t) => t.kind === 'result') ?? tabs.find((t) => t.kind === 'error') ?? tabs.at(-1)!
  const current = tabs.find((t) => t.key === selected) ?? first

  const lastStmt = exec.statements.length - 1
  return (
    <div className="results">
      <div className="row rtabs">
        {tabs.map((t) => (
          <span
            key={t.key}
            className={`rtab${t.key === current.key ? ' on' : ''}${t.kind === 'error' ? ' bad' : ''}`}
            onClick={() => setSelected(t.key)}
          >
            <TabIcon t={t} explain={explain} />
            {t.label}
            {t.kind === 'result' && (
              <span className="muted">
                · {t.rows} 行{t.capped ? '（已达上限）' : ''}
              </span>
            )}
          </span>
        ))}
        <span className="grow" />
        {running && <Spin size="small" style={{ marginRight: 8 }} />}
      </div>
      <div className="results-body">
        {current.kind === 'result' && (
          <ResultGrid
            key={current.key}
            result={exec.statements[current.stmt].results[current.result]}
            waiting={exec.waitingForMore && current.stmt === lastStmt}
            onFetchMore={onFetchMore}
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
