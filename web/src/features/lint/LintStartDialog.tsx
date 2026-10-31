// 发起检查：选一份规范，对当前对象开始检查。

import { useQuery } from '@tanstack/react-query'
import { App, Modal, Select, Tag } from 'antd'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { schemaLabel } from '../../shared/api'
import { useLintStart } from './dialog'
import { useLintTracker } from './track'

export function LintStartDialog() {
  const api = useApi()
  const { t } = useTranslation()
  const { message } = App.useApp()
  const track = useLintTracker()
  const source = useLintStart((s) => s.source)
  const close = useLintStart((s) => s.close)
  const [ruleset, setRuleset] = useState<string | undefined>(undefined)
  const [starting, setStarting] = useState(false)

  const open = source !== null
  const rulesets = useQuery({ queryKey: ['lint', 'rulesets'], queryFn: () => api.lintRulesets(), enabled: open })

  const target = source ? schemaLabel(source.schema) : ''

  async function start() {
    if (!source) return
    const id = ruleset ?? rulesets.data?.[0]?.id
    if (!id) return
    setStarting(true)
    try {
      const task = await api.startLint({ ruleset: id, source })
      close()
      void track(task, target)
    } catch (e) {
      message.error(e instanceof Error ? e.message : String(e))
    } finally {
      setStarting(false)
    }
  }

  return (
    <Modal
      title={t('lint.start.title')}
      open={open}
      onCancel={close}
      onOk={() => void start()}
      okText={t('lint.start.run')}
      confirmLoading={starting}
      okButtonProps={{ disabled: !rulesets.data?.length }}
    >
      <div style={{ margin: '12px 0', color: 'var(--text-muted, #666)' }}>
        {t('lint.start.target')}：{target}
      </div>
      <Select
        style={{ width: '100%' }}
        placeholder={t('lint.start.pick')}
        loading={rulesets.isLoading}
        value={ruleset ?? rulesets.data?.[0]?.id}
        onChange={setRuleset}
        options={(rulesets.data ?? []).map((r) => ({
          value: r.id,
          label: (
            <span>
              {r.name}
              {r.builtin ? (
                <Tag style={{ marginLeft: 8 }}>{t('lint.builtin')}</Tag>
              ) : (
                <Tag style={{ marginLeft: 8 }} color="blue">
                  {t('lint.custom')}
                </Tag>
              )}
            </span>
          ),
        }))}
      />
    </Modal>
  )
}
