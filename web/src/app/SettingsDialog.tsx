// 设置：主题（跟随系统 / 亮色 / 暗色）、结果行数上限，桌面版另有"启动时自动检查更新"。
// macOS 上没有页面菜单，主题只能在这里改。

import { Form, InputNumber, Modal, Radio, Switch } from 'antd'
import { useTranslation } from 'react-i18next'

import { DEFAULT_ROW_CAP, ROW_CAP_KEY, rowCap } from '../features/editor/runner'
import { LANG_KEY, parseLangPref } from '../shared/i18n'
import { useSetting, useSettings } from '../shared/settings'
import { desktop } from '../shared/shell'
import { THEME_KEY, parseThemePref } from '../shared/theme'
import { useUi } from './ui'
import { useShellInfo } from './useShellInfo'

export default function SettingsDialog() {
  const { t } = useTranslation()
  const { settingsOpen, setSettingsOpen } = useUi()
  const theme = parseThemePref(useSetting(THEME_KEY))
  const lang = parseLangPref(useSetting(LANG_KEY))
  useSetting(ROW_CAP_KEY)
  const info = useShellInfo()
  return (
    <Modal title="设置" open={settingsOpen} onCancel={() => setSettingsOpen(false)} footer={null} width={480}>
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
        <Form.Item label="主题">
          <Radio.Group
            value={theme}
            onChange={(e) => useSettings.getState().set(THEME_KEY, e.target.value)}
            options={[
              { value: 'system', label: '跟随系统' },
              { value: 'light', label: '亮色' },
              { value: 'dark', label: '暗色' },
            ]}
          />
        </Form.Item>
        <Form.Item label="结果行数上限" extra="一个结果集读到这么多行后停止读取">
          <InputNumber
            min={500}
            step={10000}
            value={rowCap()}
            onChange={(v) => useSettings.getState().set(ROW_CAP_KEY, v === null ? null : String(v))}
            placeholder={String(DEFAULT_ROW_CAP)}
            style={{ width: 160 }}
          />
        </Form.Item>
        {desktop && (
          <Form.Item label="启动时自动检查更新">
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
