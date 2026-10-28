// 导出 ER 图（M4b 设计文档 4.5 节）：整张图（不是当前视口）画成 SVG 或 PNG（2 倍像素）。
// 用 html-to-image 把 React Flow 的视口层画下来；桌面程序写到"另存为"选的文件，浏览器模式下载。

import { toPng, toSvg } from 'html-to-image'

export type ImageFormat = 'svg' | 'png'

export interface Bounds {
  x: number
  y: number
  width: number
  height: number
}

const PADDING = 24

/** 把视口层按整张图的范围画成 data URL。 */
export async function renderImage(viewport: HTMLElement, bounds: Bounds, format: ImageFormat, background: string): Promise<string> {
  const width = Math.ceil(bounds.width + PADDING * 2)
  const height = Math.ceil(bounds.height + PADDING * 2)
  const options = {
    backgroundColor: background,
    width,
    height,
    pixelRatio: format === 'png' ? 2 : 1,
    // 不内嵌网页字体（用系统字体），SVG 小得多
    skipFonts: true,
    style: {
      width: `${width}px`,
      height: `${height}px`,
      transform: `translate(${PADDING - bounds.x}px, ${PADDING - bounds.y}px) scale(1)`,
    },
  }
  return format === 'png' ? toPng(viewport, options) : toSvg(viewport, options)
}

/** data URL 中的内容转成 base64（SVG 的 data URL 是百分号编码的文本）。 */
export function dataUrlBase64(url: string): string {
  const comma = url.indexOf(',')
  const head = url.slice(0, comma)
  const body = url.slice(comma + 1)
  if (head.endsWith(';base64')) return body
  const bytes = new TextEncoder().encode(decodeURIComponent(body))
  let bin = ''
  for (const b of bytes) bin += String.fromCharCode(b)
  return btoa(bin)
}

/** 文件名中不能用的字符换成下划线。 */
export function imageName(name: string, format: ImageFormat): string {
  return `${name.replace(/[\\/:*?"<>|]/g, '_') || 'er'}.${format}`
}
