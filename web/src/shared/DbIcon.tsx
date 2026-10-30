// 数据库图标组件：官方 logo 画 SVG 路径，官方标识画自带颜色的图形，字标画圆角方形加缩写；颜色经 CSS 变量传入，
// 暗色主题下换用 `dark` 颜色（见 app/shell.css 的 .db-icon）。

import type { CSSProperties } from 'react'

import type { DriverId } from './api'
import { dbIcon } from './dbIcons'

export default function DbIcon({
  driver,
  variant,
  size = 16,
  title,
}: {
  driver: DriverId
  variant?: string | null
  size?: number
  title?: string
}) {
  const icon = dbIcon(driver, variant)
  if (icon.kind === 'art') {
    const gradientId = `db-icon-${variant ?? driver}`
    return (
      <svg
        className="db-icon-art"
        width={size}
        height={size}
        viewBox={icon.viewBox}
        role="img"
        aria-label={title}
        data-variant={variant ?? driver}
      >
        {title && <title>{title}</title>}
        {icon.gradient && (
          <defs>
            <linearGradient id={gradientId} x1="0" y1="1" x2="1" y2="0">
              <stop stopColor={icon.gradient[0]} />
              <stop offset="1" stopColor={icon.gradient[1]} />
            </linearGradient>
          </defs>
        )}
        {icon.shapes.map((s, i) => (
          <path
            key={i}
            d={s.d}
            fill={s.fill ?? (s.stroke ? 'none' : `url(#${gradientId})`)}
            stroke={s.stroke}
            strokeWidth={s.width}
          />
        ))}
      </svg>
    )
  }
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
