// 数据标签（M1c2 设计文档第 5 节）：浏览一张表 / 视图 / 物化视图。
// 工具栏：连接和 schema（只显示）、WHERE 过滤、刷新、停止、统计行数、导出、导入、设计表；表格沿用查询标签的结果面板，点列头排序。
// SELECT 由后端按方言生成（data-sql），经这个标签自己的会话执行（沿用 runner：续取、取消、行数上限）。
// 表的数据可以编辑（M2a 设计文档第 6 节）：改动暂存在 editStore，提交前预览，经同一个会话在事务里执行。

import {
  IconArrowBackUp,
  IconCopy,
  IconDownload,
  IconPlayerStop,
  IconRefresh,
  IconRowInsertBottom,
  IconRowRemove,
  IconSum,
  IconTableOptions,
  IconTrashX,
  IconUpload,
} from '@tabler/icons-react'
import { useQuery } from '@tanstack/react-query'
import { Alert, App, Button, Input, Tooltip } from 'antd'
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import DbIcon from '../../shared/DbIcon'
import { type ChangeSet, type Profile, errorText, exportSource, schemaLabel } from '../../shared/api'
import ResultsPanel from '../results/ResultsPanel'
import { useQueryActions } from '../editor/actions'
import { runnerFor, useTabRuntime } from '../editor/runner'
import { useExportDialog } from '../export/dialog'
import { useImportDialogs } from '../import/dialog'
import { type DataTarget, type QueryTab, useTabs } from '../editor/tabs'
import ChangePreview from './ChangePreview'
import { countRows } from './count'
import { type Pending, addRows, buildChanges, changeCount, deleteRows, emptyEdit, hasErrors, undoRow } from './edit'
import EditableGrid from './EditableGrid'
import { editInfo } from './editable'
import { useEdits } from './editStore'
import { nextOrder } from './sort'

export default function DataView({ tab, profiles }: { tab: QueryTab; profiles: Profile[] }) {
  const api = useApi()
  const { t } = useTranslation()
  const { message, modal } = App.useApp()
  const { openSql } = useQueryActions()
  const runner = runnerFor(api, tab.id)
  const rt = useTabRuntime(tab.id)
  const profile = profiles.find((p) => p.id === tab.profileId)
  const data = tab.data!
  const [filter, setFilter] = useState(data.filter ?? '')
  const [total, setTotal] = useState<string | null>(null)
  const [counting, setCounting] = useState(false)

  // ---- 编辑（M2a） ----
  const isTable = data.objectKind === 'table'
  const { data: table } = useQuery({
    queryKey: ['table-info', tab.profileId, data.schema, data.object],
    queryFn: () => api.table(tab.profileId, data.schema, data.object),
    enabled: isTable && !!profile,
  })
  const result = rt.exec.statements[0]?.results[0]
  const info = useMemo(
    () => (table && result ? editInfo(result.columns.map((c) => c.name), table) : null),
    [table, result?.columns],
  )
  const readOnly = !isTable || !info?.key
  const readOnlyReason = !isTable ? t('editor.data.edit.viewReadOnly') : info && !info.key ? t('editor.data.edit.noKey') : null
  // 人大金仓、openGauss 的 Oracle 兼容模式：空串存进去就是 NULL
  const emptyIsNull = rt.session?.compat === 'oracle'
  const edit = useEdits((s) => s.tabs[tab.id]) ?? emptyEdit
  const failedRow = useEdits((s) => s.failed[tab.id] ?? null)
  const setEdit = (next: typeof edit) => useEdits.getState().set(tab.id, next)
  const pending = changeCount(edit)
  const [selectedRows, setSelectedRows] = useState<number[]>([])
  const [preview, setPreview] = useState<{ set: ChangeSet; rowOf: number[]; sql: { index: number; sql: string }[] } | null>(
    null,
  )
  const [applying, setApplying] = useState(false)
  const loaded = result?.rows.length ?? 0

  /** 有未提交的修改时先确认放弃，再做 `then`。 */
  const confirmDiscard = (then: () => void) => {
    if (!pending) return then()
    modal.confirm({
      title: t('editor.data.edit.discardTitle'),
      content: t('editor.data.edit.discardContent'),
      okText: t('editor.data.edit.discardOk'),
      okType: 'danger',
      onOk: () => {
        useEdits.getState().clear(tab.id)
        then()
      },
    })
  }

  const load = async (d: DataTarget) => {
    try {
      const sql = await api.dataSql(tab.profileId, d.schema, { object: d.object, filter: d.filter, order: d.order })
      await runner.rerun(sql)
    } catch (e) {
      message.error((e as Error).message)
    }
  }

  /** 改过滤条件或排序：保存到标签，重新读取。 */
  const apply = (patch: Pick<DataTarget, 'filter' | 'order'>) =>
    confirmDiscard(() => {
      const next = { ...data, ...patch }
      useTabs.getState().setData(tab.id, patch)
      if ('filter' in patch) setTotal(null)
      void load(next)
    })

  // 第一次显示时（以及连接从已删除恢复时）读取；切换回来时保留已读取的结果
  useEffect(() => {
    if (profile && !rt.submission && !rt.running) void load(data)
  }, [profile?.id])

  const count = async () => {
    setCounting(true)
    try {
      const sql = await api.dataSql(tab.profileId, data.schema, { object: data.object, filter: data.filter, count: true })
      setTotal(await countRows(api, tab.profileId, sql, data.schema))
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setCounting(false)
    }
  }

  /** 复制行：以第一行选中的行为底稿新增一行，行标识列不复制。 */
  const copyRow = () => {
    const row = selectedRows[0]
    if (row === undefined || !info?.key || !result) return
    const template: Record<number, Pending> = {}
    info.columns.forEach((c, col) => {
      if (!c.editable || info.key!.includes(col)) return
      const cell = row < loaded ? { value: result.rows[row][col] } : edit.inserts[row - loaded]?.[col]
      if (cell) template[col] = cell
    })
    setEdit(addRows(edit, 1, template))
  }

  const commit = async () => {
    if (!info?.key || !result) return
    if (hasErrors(edit)) {
      message.error(t('editor.data.edit.hasErrors'))
      return
    }
    const { changes, rowOf } = buildChanges(edit, loaded, result.rows, result.columns, info.key)
    const set: ChangeSet = { schema: data.schema, table: data.object, changes }
    try {
      setPreview({ set, rowOf, sql: await api.previewChanges(tab.profileId, set) })
    } catch (e) {
      message.error((e as Error).message)
    }
  }

  const execute = async () => {
    if (!preview) return
    setApplying(true)
    try {
      // 会话可能还在等续取的结果：先停下
      await runner.stop()
      const session = rt.session
      if (!session) {
        message.error(t('editor.data.edit.noSession'))
        return
      }
      const out = await api.applyChanges(session.id, preview.set)
      if (out.failed) {
        useEdits.getState().setFailed(tab.id, preview.rowOf[out.failed.index] ?? null)
        message.error(errorText(out.failed.error), 8)
      } else {
        useEdits.getState().clear(tab.id)
        message.success(t('editor.data.edit.applied', { count: out.applied }))
        void load(data)
      }
      setPreview(null)
      void runner.refreshSession()
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setApplying(false)
    }
  }

  return (
    <div className="query">
      <div className="row toolbar">
        {profile && <DbIcon driver={profile.driver} variant={rt.session?.variant ?? profile.variant} />}
        <span>{profile?.name}</span>
        <span className="muted">{schemaLabel(data.schema)}</span>
        <span className="sep" />
        <Input
          size="small"
          allowClear
          style={{ flex: 1, fontFamily: 'var(--mono)' }}
          placeholder={t('editor.data.filterPlaceholder')}
          value={filter}
          disabled={!profile}
          onChange={(e) => {
            setFilter(e.target.value)
            // 清空即取消过滤
            if (!e.target.value && data.filter) apply({ filter: undefined })
          }}
          onPressEnter={() => apply({ filter: filter.trim() || undefined })}
        />
        <Tooltip title={t('editor.data.refresh')} mouseEnterDelay={0.6}>
          <Button type="text" size="small" disabled={!profile} onClick={() => confirmDiscard(() => void load(data))}>
            <IconRefresh size={16} />
          </Button>
        </Tooltip>
        <Tooltip title={t('editor.data.stop')} mouseEnterDelay={0.6}>
          <Button type="text" size="small" disabled={!rt.running} onClick={() => runner.cancel()}>
            <IconPlayerStop size={16} />
          </Button>
        </Tooltip>
        <Button size="small" icon={<IconSum size={14} />} loading={counting} disabled={!profile} onClick={() => void count()}>
          {t('editor.data.count')}
        </Button>
        <Button
          size="small"
          icon={<IconDownload size={14} />}
          disabled={!profile}
          onClick={() =>
            useExportDialog.getState().open({
              profileId: tab.profileId,
              source: exportSource(data.schema, data.object, data.filter, data.order),
              baseName: data.object,
              table: `${schemaLabel(data.schema)}.${data.object}`,
              inTransaction: false,
            })
          }
        >
          {t('results.export.button')}
        </Button>
        {isTable && (
          <Button
            size="small"
            icon={<IconUpload size={14} />}
            disabled={!profile}
            onClick={() =>
              useImportDialogs
                .getState()
                .openImport({ profileId: tab.profileId, schema: data.schema, table: data.object, create: false })
            }
          >
            {t('editor.data.import')}
          </Button>
        )}
        {isTable && (
          <Button
            size="small"
            icon={<IconTableOptions size={14} />}
            disabled={!profile}
            onClick={() => useTabs.getState().openDesign(tab.profileId, { schema: data.schema, table: data.object })}
          >
            {t('editor.data.design')}
          </Button>
        )}
        {total !== null && <span className="muted">{t('editor.data.total', { count: Number(total) })}</span>}
      </div>
      {/* 表显示编辑按钮；视图和没有行标识的表显示只读原因 */}
      {(isTable || readOnlyReason) && (
        <div className="row toolbar">
          {readOnlyReason ? (
            <span className="muted">{readOnlyReason}</span>
          ) : (
            <>
              <Button size="small" type="text" icon={<IconRowInsertBottom size={16} />} disabled={readOnly} onClick={() => setEdit(addRows(edit))}>
                {t('editor.data.edit.addRow')}
              </Button>
              <Button size="small" type="text" icon={<IconCopy size={16} />} disabled={readOnly || !selectedRows.length} onClick={copyRow}>
                {t('editor.data.edit.copyRow')}
              </Button>
              <Button
                size="small"
                type="text"
                icon={<IconRowRemove size={16} />}
                disabled={readOnly || !selectedRows.length}
                onClick={() => setEdit(deleteRows(edit, loaded, selectedRows))}
              >
                {t('editor.data.edit.deleteRows')}
              </Button>
              <Button
                size="small"
                type="text"
                icon={<IconArrowBackUp size={16} />}
                disabled={!pending || !selectedRows.length}
                onClick={() => setEdit([...selectedRows].sort((a, b) => b - a).reduce((s, r) => undoRow(s, loaded, r), edit))}
              >
                {t('editor.data.edit.undo')}
              </Button>
              <Button size="small" type="text" icon={<IconTrashX size={16} />} disabled={!pending} onClick={() => confirmDiscard(() => {})}>
                {t('editor.data.edit.discard')}
              </Button>
              <span className="grow" />
              {emptyIsNull && <span className="muted">{t('editor.data.edit.emptyIsNull')}</span>}
              <Button size="small" type="primary" disabled={!pending || readOnly} onClick={() => void commit()}>
                {t('editor.data.edit.commit', { count: pending })}
              </Button>
            </>
          )}
        </div>
      )}
      {!profile && <Alert type="error" banner message={t('editor.query.profileDeleted')} />}
      <div className="grow" style={{ minHeight: 0 }}>
        <ResultsPanel
          rt={rt}
          onFetchMore={() => runner.fetchMore()}
          sort={data.order ?? null}
          onSort={(column) => apply({ order: nextOrder(data.order, column) })}
          renderResult={
            isTable && info
              ? (r, waiting) => (
                  <EditableGrid
                    result={r}
                    waiting={waiting}
                    onFetchMore={() => runner.fetchMore()}
                    sort={data.order ?? null}
                    onSort={(column) => apply({ order: nextOrder(data.order, column) })}
                    columns={info.columns}
                    readOnly={readOnly}
                    edit={edit}
                    onEdit={setEdit}
                    failedRow={failedRow}
                    onSelectRows={setSelectedRows}
                  />
                )
              : undefined
          }
        />
      </div>
      <div className="row muted" style={{ padding: '2px 8px', gap: 8, fontSize: 'var(--fs-sm)', borderTop: '1px solid var(--border)' }}>
        <span>{t('editor.data.loaded', { count: loaded })}</span>
        <span className="grow" />
        <span>{t('editor.data.sortHint')}</span>
      </div>
      <ChangePreview
        statements={preview?.sql ?? null}
        prod={profile?.environment === 'prod'}
        busy={applying}
        onCancel={() => setPreview(null)}
        onOpenInTab={() => {
          if (!preview) return
          openSql(preview.sql.map((s) => `${s.sql};`).join('\n'), tab.profileId)
          setPreview(null)
        }}
        onExecute={() => void execute()}
      />
    </div>
  )
}
