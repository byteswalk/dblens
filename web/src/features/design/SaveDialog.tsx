// 保存确认框（M3 设计文档 7.4 节）：变更摘要、完整的 SQL、"用重建方式执行"开关（确定不支持时强制打开）、提示、
// 生产环境的横幅；执行、复制 SQL、在新查询标签中打开、取消。执行时后端用草稿重新生成语句，返回任务 id。

import { useQuery } from '@tanstack/react-query'
import { Alert, App, Button, Modal, Spin, Switch } from 'antd'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { type DesignRequest, type Profile, schemaLabel } from '../../shared/api'
import { useQueryActions } from '../editor/actions'
import SqlBox from './SqlBox'
import { describeChange, scriptOf } from './summary'

interface Props {
  profile: Profile
  request: DesignRequest
  onRebuild(on: boolean): void
  onReload(): void
  onClose(): void
  /** 已提交执行：任务 id。 */
  onStarted(taskId: string): void
}

export default function SaveDialog({ profile, request, onRebuild, onReload, onClose, onStarted }: Props) {
  const api = useApi()
  const { t } = useTranslation()
  const { message } = App.useApp()
  const { openSql } = useQueryActions()
  const [starting, setStarting] = useState(false)
  const { data: p, error, isFetching } = useQuery({
    queryKey: ['design-save', profile.id, JSON.stringify(request)],
    queryFn: () => api.previewDesign(profile.id, request),
    gcTime: 0,
    retry: false,
  })
  const script = p ? scriptOf(p.statements, profile.driver) : ''
  const blocked =
    !p || p.errors.length > 0 || p.conflict || !!p.rebuild.refused || !!p.denied || p.statements.length === 0
  const name = `${schemaLabel(request.schema)}.${request.draft.name}`
  const execute = async () => {
    setStarting(true)
    try {
      onStarted(await api.applyDesign(profile.id, request))
      onClose()
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setStarting(false)
    }
  }
  const danger = profile.environment === 'prod' || p?.rebuild.active

  return (
    <Modal
      open
      width={900}
      title={t('design.save.title', { name })}
      onCancel={onClose}
      footer={[
        <Button key="cancel" onClick={onClose}>
          {t('design.save.cancel')}
        </Button>,
        <Button
          key="copy"
          disabled={!script}
          onClick={() => void navigator.clipboard.writeText(script).then(() => message.success(t('design.save.copied')))}
        >
          {t('design.save.copy')}
        </Button>,
        <Button
          key="open"
          disabled={!script}
          onClick={() => {
            openSql(script, profile.id)
            onClose()
          }}
        >
          {t('design.save.openInTab')}
        </Button>,
        <Button key="run" type="primary" danger={danger} disabled={blocked} loading={starting} onClick={() => void execute()}>
          {t('design.save.execute')}
        </Button>,
      ]}
    >
      {isFetching && !p ? (
        <Spin style={{ margin: 48 }} />
      ) : error ? (
        <Alert type="error" message={(error as Error).message} />
      ) : p ? (
        <div className="design-save">
          {profile.environment === 'prod' && <Alert type="error" banner message={t('design.save.prod')} />}
          {p.conflict && (
            <Alert
              type="error"
              message={t('design.save.conflict')}
              action={
                <Button size="small" onClick={onReload}>
                  {t('design.toolbar.reload')}
                </Button>
              }
            />
          )}
          {p.errors.length > 0 && <Alert type="error" message={t('design.sql.errors', { count: p.errors.length })} />}
          {p.denied && <Alert type="error" message={t('design.save.denied', { reason: p.denied })} />}
          {p.rebuild.refused && <Alert type="error" message={t('design.sql.refused', { reason: p.rebuild.refused })} />}
          {request.name !== null && (
            <div className="row">
              <Switch
                size="small"
                checked={p.rebuild.active}
                disabled={p.rebuild.reasons.length > 0 || isFetching}
                onChange={onRebuild}
              />
              <span>{t('design.save.rebuildSwitch')}</span>
              {p.rebuild.reasons.length > 0 && <span className="muted">{t('design.save.rebuildForced')}</span>}
            </div>
          )}
          {p.rebuild.active && (
            <Alert
              type="warning"
              message={t('design.sql.rebuild')}
              description={p.rebuild.reasons.length ? p.rebuild.reasons.join(' · ') : undefined}
            />
          )}
          {[...p.warnings, ...p.confirm].map((w) => (
            <Alert key={w} type="warning" showIcon message={w} />
          ))}
          <Alert type="info" showIcon message={t(p.transactional ? 'design.sql.transactional' : 'design.sql.autocommit')} />
          <h4>{t('design.save.changes')}</h4>
          <ul className="design-changes">
            {p.changes.flatMap(describeChange).map((line, i) => (
              <li key={i}>{line}</li>
            ))}
          </ul>
          <h4>{t('design.save.sql')}</h4>
          <SqlBox sql={script} driver={profile.driver} height={260} />
        </div>
      ) : null}
    </Modal>
  )
}
