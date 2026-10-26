// 数据库图标组件：官方 logo 画 SVG 路径，字标画圆角方形加缩写；颜色经 CSS 变量传入，
// 暗色主题下换用 `dark` 颜色（见 app/shell.css 的 .db-icon）。

import type { CSSProperties } from 'react'

import type { DriverId } from './api'
import { dbIcon } from './dbIcons'

export default function DbIcon({
  driver,
  variant,
  size = 14,
  title,
}: {
  driver: DriverId
  variant?: string | null
  size?: number
  title?: string
}) {
  const icon = dbIcon(driver, variant)
  const style = {
    '--db-icon': icon.color,
    '--db-icon-dark': icon.kind === 'logo' ? (icon.dark ?? icon.color) : icon.color,
  } as CSSProperties
  return (
    <svg
      className="db-icon"
      width={size}
      height={size}
      viewBox="0 0 24 24"
      style={style}
      role="img"
      aria-label={title}
      data-variant={variant ?? driver}
    >
      {title && <title>{title}</title>}
      {icon.kind === 'logo' ? (
        <path d={icon.path} />
      ) : (
        <>
          <rect x="0.5" y="0.5" width="23" height="23" rx="5" />
          <text
            x="12"
            y="12.5"
            textAnchor="middle"
            dominantBaseline="central"
            fontSize={icon.text.length > 2 ? 9 : 11}
            fontWeight="700"
            fontFamily="system-ui, sans-serif"
          >
            {icon.text}
          </text>
        </>
      )}
    </svg>
  )
}
