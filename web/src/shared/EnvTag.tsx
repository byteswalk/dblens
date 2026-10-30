// 连接环境的文字标签（开发 / 测试 / 生产 / 未设环境）。用文字而不是色点，免得被看成连接状态灯。

import { useTranslation } from 'react-i18next'

import type { Environment } from './api'

export default function EnvTag({ env }: { env?: Environment | null }) {
  const { t } = useTranslation()
  const value = env ?? 'none'
  return <span className={`env-tag ${value}`}>{t(`tree.env.${value}`)}</span>
}
