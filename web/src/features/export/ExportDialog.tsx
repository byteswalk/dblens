// 导出对话框（M1c3 设计文档 4.2 节）：选格式和选项；桌面版再弹系统"另存为"。提交后右下角的通知显示进度、可取消，
// 完成后桌面版可以打开所在文件夹，浏览器模式自动下载。通知在对话框关闭后继续轮询（对话框挂在外壳上，不卸载）。

import { Alert, App, Button, Checkbox, Form, Input, InputNumber, Modal, Radio, Select } from 'antd'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { type ExportFormat, type ExportOptions, errorText } from '../../shared/api'
import { desktop } from '../../shared/shell'
import { withVault } from '../connections/vault'
import { useExportDialog } from './dialog'
import { FORMATS, baseName, defaultFileName, defaultOptions, fileFilter } from './options'

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))

export default function ExportDialog() {
  const api = useApi()
  const { t } = useTranslation()
  const { notification } = App.useApp()
  const { draft, close } = useExportDialog()
  // 格式和选项在两次导出之间保留（目标表除外）
  const [format, setFormat] = useState<ExportFormat>('csv')
  const [options, setOptions] = useState<ExportOptions>(defaultOptions())
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    if (!draft) return
    setOptions((o) => ({ ...o, table: draft.table ?? 'table_name' }))
    setError(null)
  }, [draft])
  const set = (patch: Partial<ExportOptions>) => setOptions((o) => ({ ...o, ...patch }))

  /** 轮询任务直到结束，用同一个 key 的通知显示进度和结果。 */
  const track = async (id: string, name: string) => {
    const key = `export-${id}`
    await sleep(300)
    for (;;) {
      const st = await api.exportStatus(id).catch(() => null)
      if (!st) return
      if (st.state === 'running') {
        notification.open({
          key,
          message: t('results.export.running', { name }),
          description: t('results.export.progress', { count: st.rows }),
          duration: 0,
          actions: (
            <Button size="small" onClick={() => void api.cancelExport(id)}>
              {t('results.export.cancel')}
            </Button>
          ),
        })
        await sleep(1000)
        continue
      }
      if (st.state === 'done') {
        const lines = [t('results.export.doneRows', { count: st.rows })]
        if (st.warnings.lob_truncated) lines.push(t('results.export.lobTruncated', { count: st.warnings.lob_truncated }))
        if (st.warnings.cell_truncated) lines.push(t('results.export.cellTruncated', { count: st.warnings.cell_truncated }))
        const path = st.path
        notification.success({
          key,
          message: t('results.export.done', { name }),
          description: lines.map((l) => <div key={l}>{l}</div>),
          duration: path ? 0 : 8,
          actions:
            desktop && path ? (
              <Button size="small" onClick={() => void desktop?.revealInFolder(path)}>
                {t('results.export.reveal')}
              </Button>
            ) : undefined,
        })
        if (!path) {
          // 浏览器模式：下载（下载后服务端删除临时文件和任务）
          const a = document.createElement('a')
          a.href = api.exportFileUrl(id)
          a.download = name
          a.click()
        }
      } else if (st.state === 'failed') {
        notification.error({
          key,
          message: t('results.export.failed', { name }),
          description: st.error ? errorText(st.error) : null,
          duration: 0,
        })
        void api.cancelExport(id)
      } else {
        notification.info({ key, message: t('results.export.cancelled', { name }), duration: 4 })
        void api.cancelExport(id)
      }
      return
    }
  }

  const submit = async () => {
    if (!draft) return
    let name = defaultFileName(draft.baseName, format)
    let path: string | null = null
    if (desktop) {
      path = await desktop.saveDialog(name, [fileFilter(format, t(`results.export.formats.${format}`))])
      if (!path) return
      name = baseName(path)
    }
    setBusy(true)
    setError(null)
    try {
      const req = { profile_id: draft.profileId, source: draft.source, format, options, path, file_name: name }
      const id = await withVault(api, () => api.startExport(req))
      close()
      void track(id, name)
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Modal
      open={draft !== null}
      title={t('results.export.title')}
      okText={t('results.export.ok')}
      confirmLoading={busy}
      onOk={() => void submit()}
      onCancel={close}
      destroyOnHidden
    >
      {draft?.inTransaction && <Alert type="warning" showIcon message={t('results.export.uncommitted')} style={{ marginBottom: 12 }} />}
      <Form layout="vertical">
        <Form.Item label={t('results.export.format')}>
          <Radio.Group
            value={format}
            onChange={(e) => setFormat(e.target.value as ExportFormat)}
            options={FORMATS.map((f) => ({ value: f, label: t(`results.export.formats.${f}`) }))}
          />
        </Form.Item>
        {format === 'csv' && (
          <>
            <Form.Item label={t('results.export.delimiter')}>
              <Select
                value={options.delimiter}
                onChange={(delimiter) => set({ delimiter })}
                style={{ width: 160 }}
                options={[
                  { value: ',', label: t('results.export.delimiters.comma') },
                  { value: '\t', label: t('results.export.delimiters.tab') },
                  { value: ';', label: t('results.export.delimiters.semicolon') },
                ]}
              />
            </Form.Item>
            <Form.Item label={t('results.export.nullText')}>
              <Select
                value={options.null_text}
                onChange={(null_text) => set({ null_text })}
                style={{ width: 160 }}
                options={[
                  { value: '', label: t('results.export.nullEmpty') },
                  { value: 'NULL', label: 'NULL' },
                ]}
              />
            </Form.Item>
            <Checkbox checked={options.header} onChange={(e) => set({ header: e.target.checked })}>
              {t('results.export.header')}
            </Checkbox>
            <br />
            <Checkbox checked={options.bom} onChange={(e) => set({ bom: e.target.checked })}>
              {t('results.export.bom')}
            </Checkbox>
          </>
        )}
        {format === 'insert' && (
          <>
            <Form.Item label={t('results.export.table')}>
              <Input value={options.table} onChange={(e) => set({ table: e.target.value })} />
            </Form.Item>
            <Form.Item label={t('results.export.rowsPerInsert')}>
              <InputNumber
                min={1}
                max={1000}
                value={options.rows_per_insert}
                onChange={(v) => set({ rows_per_insert: v ?? 1 })}
              />
            </Form.Item>
          </>
        )}
        {format === 'xlsx' && <div className="muted">{t('results.export.xlsxNote')}</div>}
        {format === 'json' && <div className="muted">{t('results.export.jsonNote')}</div>}
      </Form>
      {error && <Alert type="error" showIcon message={error} style={{ marginTop: 12 }} />}
    </Modal>
  )
}
