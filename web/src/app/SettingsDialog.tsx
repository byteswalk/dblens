// 设置：主题（跟随系统 / 亮色 / 暗色）、结果行数上限、语法检查、历史，桌面版另有"启动时自动检查更新"。
// macOS 上没有页面菜单，主题只能在这里改。

import { useQueryClient } from '@tanstack/react-query'
import { App, Button, Form, InputNumber, Modal, Radio, Switch } from 'antd'
import { useTranslation } from 'react-i18next'

import { DEFAULT_ROW_CAP, ROW_CAP_KEY, rowCap } from '../features/editor/runner'
import { SYNTAX_KEY, syntaxEnabled } from '../features/sql/syntax'
import { LANG_KEY, parseLangPref } from '../shared/i18n'
import { useSetting, useSettings } from '../shared/settings'
import { desktop } from '../shared/shell'
import { THEME_KEY, parseThemePref } from '../shared/theme'
import { useApi } from './context'
import { useUi } from './ui'
import { useShellInfo } from './useShellInfo'

/** 历史设置（由服务端读取）：值为 off 时不记录；保留条数，默认 10000。 */
const HISTORY_ENABLED_KEY = 'ui.history.enabled'
const HISTORY_LIMIT_KEY = 'ui.history.limit'

export default function SettingsDialog() {
  const { t } = useTranslation()
  const { settingsOpen, setSettingsOpen } = useUi()
  const theme = parseThemePref(useSetting(THEME_KEY))
  const lang = parseLangPref(useSetting(LANG_KEY))
  useSetting(ROW_CAP_KEY)
  const syntax = syntaxEnabled(useSetting(SYNTAX_KEY))
  const info = useShellInfo()
  const api = useApi()
  const qc = useQueryClient()
  const { message, modal } = App.useApp()
  const historyOn = useSetting(HISTORY_ENABLED_KEY) !== 'off'
  const historyLimit = Number(useSetting(HISTORY_LIMIT_KEY)) || null
  const clearHistory = () =>
    modal.confirm({
      title: t('app.settings.clearHistoryConfirm'),
      okButtonProps: { danger: true },
      onOk: async () => {
        try {
          await api.clearHistory()
          await qc.invalidateQueries({ queryKey: ['history'] })
        } catch (e) {
          message.error((e as Error).message)
        }
      },
    })
  return (
    <Modal title={t('app.settings.title')} open={settingsOpen} onCancel={() => setSettingsOpen(false)} footer={null} width={480}>
      <Form layout="horizontal" labelCol={{ span: 8 }} style={{ marginTop: 16 }}>
        <Form.Item label={t('app.settings.language')} extra={t('app.settings.languageHint')}>
          <Radio.Group
            value={lang}
            onChange={(e) => useSettings.getState().set(LANG_KEY, e.target.value)}
            options={[
              { value: 'auto', label: t('common.lang.auto') },
              { value: 'zh-CN', label: t('common.lang.zh-CN') },
              { value: 'en', label: t('common.lang.en') },
            ]}
          />
        </Form.Item>
        <Form.Item label={t('app.settings.theme')}>
          <Radio.Group
            value={theme}
            onChange={(e) => useSettings.getState().set(THEME_KEY, e.target.value)}
            options={[
              { value: 'system', label: t('app.theme.system') },
              { value: 'light', label: t('app.theme.light') },
              { value: 'dark', label: t('app.theme.dark') },
            ]}
          />
        </Form.Item>
        <Form.Item label={t('app.settings.rowCap')} extra={t('app.settings.rowCapHint')}>
          <InputNumber
            min={500}
            step={10000}
            value={rowCap()}
            onChange={(v) => useSettings.getState().set(ROW_CAP_KEY, v === null ? null : String(v))}
            placeholder={String(DEFAULT_ROW_CAP)}
            style={{ width: 160 }}
          />
        </Form.Item>
        <Form.Item label={t('app.settings.syntaxCheck')} extra={t('app.settings.syntaxCheckHint')}>
          <Switch checked={syntax} onChange={(on) => useSettings.getState().set(SYNTAX_KEY, on ? null : 'off')} />
        </Form.Item>
        <Form.Item label={t('app.settings.history')} extra={t('app.settings.historyHint')}>
          <div className="row" style={{ gap: 8 }}>
            <Switch
              checked={historyOn}
              onChange={(on) => useSettings.getState().set(HISTORY_ENABLED_KEY, on ? null : 'off')}
            />
            <InputNumber
              min={100}
              max={1_000_000}
              step={1000}
              value={historyLimit}
              placeholder="10000"
              onChange={(v) => useSettings.getState().set(HISTORY_LIMIT_KEY, v === null ? null : String(v))}
              style={{ width: 110 }}
            />
            <span className="muted">{t('app.settings.historyLimitUnit')}</span>
            <Button size="small" danger onClick={clearHistory}>
              {t('app.settings.clearHistory')}
            </Button>
          </div>
        </Form.Item>
        {desktop && (
          <Form.Item label={t('app.settings.autoCheckUpdate')}>
            <Switch
              checked={info?.auto_check_update ?? true}
              onChange={(on) => desktop?.setAutoCheckUpdate(on).catch(console.warn)}
            />
          </Form.Item>
        )}
      </Form>
    </Modal>
  )
}
