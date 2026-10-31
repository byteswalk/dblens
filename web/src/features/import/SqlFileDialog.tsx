// 执行 SQL 文件（M2b 设计文档 3.4 节）：选文件、编码、目标 schema、出错时停止或继续、整体一个事务。
// 提交后先扫描，需要确认时弹确认框（track.tsx），之后逐条执行。

import { useQuery } from '@tanstack/react-query'
import { Alert, Checkbox, Modal, Radio, Select } from 'antd'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { type TextEncoding, sessionSchema } from '../../shared/api'
import { hasCatalogs } from '../../shared/schemaOptions'
import { withVault } from '../connections/vault'
import FileField, { type PickedFile } from './FileField'
import { useImportDialogs } from './dialog'
import { useTaskTracker } from './track'

export default function SqlFileDialog() {
  const api = useApi()
  const { t } = useTranslation()
  const target = useImportDialogs((s) => s.sqlTarget)
  const close = useImportDialogs((s) => s.closeSqlFile)
  const track = useTaskTracker()
  const profileId = target?.profileId ?? ''
  const [file, setFile] = useState<PickedFile | null>(null)
  const [encoding, setEncoding] = useState<TextEncoding>('auto')
  const [schema, setSchema] = useState<string | null>(null)
  const [continueOnError, setContinueOnError] = useState(false)
  const [transaction, setTransaction] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!target) return
    setFile(null)
    setSchema(target.schema === null ? null : sessionSchema(target.schema))
    setError(null)
  }, [target])

  const { data: profiles = [] } = useQuery({ queryKey: ['profiles'], queryFn: () => api.profiles() })
  const profile = profiles.find((p) => p.id === profileId)
  const schemas = useQuery({
    queryKey: ['schemas', profileId, profile?.driver],
    queryFn: () => withVault(api, () => (profile && hasCatalogs(profile.driver) ? api.catalogs(profileId) : api.schemas(profileId))),
    enabled: !!target && !!profile,
  })

  const submit = async () => {
    if (!target || !file) return
    setBusy(true)
    setError(null)
    try {
      const id = await withVault(api, () =>
        api.startSqlFile({
          profile_id: profileId,
          source: file.source,
          encoding,
          schema,
          continue_on_error: continueOnError && !transaction,
          transaction,
        }),
      )
      close()
      void track(id, {
        title: t('import.sql.taskTitle', { file: file.name, connection: profile?.name ?? '' }),
        profileId,
        schema,
        refreshTree: true,
      })
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Modal
      open={target !== null}
      title={t('import.sql.title')}
      okText={t('import.sql.start')}
      okButtonProps={{ disabled: !file, danger: profile?.environment === 'prod' }}
      confirmLoading={busy}
      onOk={() => void submit()}
      onCancel={close}
      destroyOnHidden
      width={600}
    >
      <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
        <FileField
          value={file}
          onChange={setFile}
          filters={[
            { name: 'SQL', extensions: ['sql', 'txt'] },
            { name: t('import.file.all'), extensions: ['*'] },
          ]}
        />
        <div className="row" style={{ gap: 16, flexWrap: 'wrap' }}>
          <span>
            {t('import.file.encoding')}{' '}
            <Select
              size="small"
              value={encoding}
              style={{ width: 120 }}
              onChange={setEncoding}
              options={[
                { value: 'auto', label: t('import.file.auto') },
                { value: 'utf8', label: 'UTF-8' },
                { value: 'gbk', label: 'GBK' },
              ]}
            />
          </span>
          <span>
            {t('import.target.schema')}{' '}
            <Select
              size="small"
              showSearch
              allowClear
              value={schema ?? undefined}
              placeholder={t('import.sql.defaultSchema')}
              style={{ width: 200 }}
              onChange={(s) => setSchema(s ?? null)}
              options={(schemas.data ?? []).map((s) => ({ value: s.name, label: s.name }))}
            />
          </span>
        </div>
        <div>
          <div className="form-label">{t('import.sql.onError')}</div>
          <Radio.Group
            value={continueOnError && !transaction ? 'continue' : 'stop'}
            disabled={transaction}
            onChange={(e) => setContinueOnError(e.target.value === 'continue')}
            options={[
              { value: 'stop', label: t('import.options.stop') },
              { value: 'continue', label: t('import.sql.continue') },
            ]}
          />
        </div>
        <div>
          <Checkbox checked={transaction} onChange={(e) => setTransaction(e.target.checked)}>
            {t('import.sql.transaction')}
          </Checkbox>
          {transaction && <div className="muted">{t('import.sql.transactionNote')}</div>}
        </div>
        <div className="muted">{t('import.sql.scanNote')}</div>
        {profile?.environment === 'prod' && <Alert type="error" showIcon message={t('import.sql.prodNote')} />}
        {error && <Alert type="error" showIcon message={error} />}
      </div>
    </Modal>
  )
}
