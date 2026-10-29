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

/** 把视口层按整张图的范围画成 data URL。PNG 默认 2 倍像素。 */
export async function renderImage(
  viewport: HTMLElement,
  bounds: Bounds,
  format: ImageFormat,
  background: string,
  ratio = 2,
): Promise<string> {
  const width = Math.ceil(bounds.width + PADDING * 2)
  const height = Math.ceil(bounds.height + PADDING * 2)
  const options = {
    backgroundColor: background,
    width,
    height,
    pixelRatio: format === 'png' ? ratio : 1,
    // 不内嵌网页字体（用系统字体），SVG 小得多
    skipFonts: true,
    // 连线手柄和连线的点击区域是操作用的，不画进图片（表多时也快得多）
    filter: (n: HTMLElement) =>
      !n.classList?.contains('react-flow__handle') && !n.classList?.contains('react-flow__edge-interaction'),
    style: {
      width: `${width}px`,
      height: `${height}px`,
      transform: `translate(${PADDING - bounds.x}px, ${PADDING - bounds.y}px) scale(1)`,
    },
  }
  if (format === 'png') return toPng(viewport, options)
  const url = await toSvg(viewport, options)
  const comma = url.indexOf(',')
  return url.slice(0, comma + 1) + encodeURIComponent(slimSvg(decodeURIComponent(url.slice(comma + 1))))
}

// html-to-image 给每个元素抄下全部计算样式（含几百个 CSS 变量），几张表的 SVG 就有几 MB。
// 去掉其中不影响显示的声明：CSS 变量（值已算进各属性）；会继承的属性和上级相同的；
// 不继承的属性和同名元素的默认值相同的。
const INHERITED = new Set(
  (
    'color,cursor,direction,font,font-family,font-feature-settings,font-kerning,font-optical-sizing,font-palette,' +
    'font-size,font-size-adjust,font-stretch,font-style,font-synthesis,font-synthesis-small-caps,font-synthesis-style,' +
    'font-synthesis-weight,font-variant,font-variant-alternates,font-variant-caps,font-variant-east-asian,' +
    'font-variant-emoji,font-variant-ligatures,font-variant-numeric,font-variant-position,font-variation-settings,' +
    'font-weight,hyphens,letter-spacing,line-break,line-height,list-style,list-style-image,list-style-position,' +
    'list-style-type,orphans,overflow-wrap,quotes,tab-size,text-align,text-align-last,text-indent,text-justify,' +
    'text-rendering,text-shadow,text-transform,text-wrap,text-wrap-mode,text-wrap-style,text-underline-position,' +
    'text-underline-offset,text-emphasis-color,text-emphasis-position,text-emphasis-style,text-size-adjust,visibility,' +
    'white-space,white-space-collapse,widows,word-break,word-spacing,word-wrap,writing-mode,text-orientation,' +
    'caret-color,accent-color,color-scheme,color-interpolation,color-interpolation-filters,color-rendering,' +
    'dominant-baseline,fill,fill-opacity,fill-rule,clip-rule,image-rendering,marker,marker-start,marker-mid,' +
    'marker-end,paint-order,pointer-events,shape-rendering,stroke,stroke-dasharray,stroke-dashoffset,stroke-linecap,' +
    'stroke-linejoin,stroke-miterlimit,stroke-opacity,stroke-width,text-anchor,border-collapse,border-spacing,' +
    'caption-side,empty-cells,ruby-position,speak,print-color-adjust,math-depth,math-shift,math-style,' +
    'forced-color-adjust,hanging-punctuation,text-spacing-trim,text-autospace,user-select,interpolate-size,' +
    'font-smoothing,text-fill-color,text-stroke,text-stroke-color,text-stroke-width,text-security,locale,' +
    'rtl-ordering,user-modify,hyphenate-character,tap-highlight-color,text-decorations-in-effect,' +
    'border-horizontal-spacing,border-vertical-spacing,box-direction,highlight'
  ).split(','),
)

const inherited = (name: string) => INHERITED.has(name) || (name.startsWith('-webkit-') && INHERITED.has(name.slice(8)))

// 图片中用不到的：交互相关的属性；逻辑属性（图是从左到右横排的，和对应的物理属性重复）
const DROPPED = /^(-webkit-)?(app-region|cursor|pointer-events|user-select|caret-color|touch-action|will-change|transition|animation|scroll-|overscroll-|(min-|max-)?(block|inline)-size|inset-(block|inline)|(border|margin|padding|scroll-margin|scroll-padding)-(block|inline))/

// 默认值是黑色但不随文字颜色变化的属性
const LITERAL_COLOR = new Set(['flood-color', 'stop-color', 'lighting-color'])

/** 拆开 style 属性中的声明（引号、括号中的分号不拆）。 */
export function splitStyle(css: string): [string, string][] {
  const out: [string, string][] = []
  let depth = 0
  let quote: string | null = null
  let cur = ''
  const push = () => {
    const i = cur.indexOf(':')
    if (i > 0) out.push([cur.slice(0, i).trim(), cur.slice(i + 1).trim()])
    cur = ''
  }
  for (const ch of css) {
    if (quote) {
      if (ch === quote) quote = null
    } else if (ch === '"' || ch === "'") quote = ch
    else if (ch === '(') depth++
    else if (ch === ')') depth--
    else if (ch === ';' && depth === 0) {
      push()
      continue
    }
    cur += ch
  }
  push()
  return out
}

// 默认值中的默认文字颜色换成某个文字颜色后的结果：同名元素、同一文字颜色只算一次
const colored = new WeakMap<Map<string, string>, Map<string, Map<string, string>>>()

function withColor(defaults: Map<string, string>, from: string, to: string): Map<string, string> {
  let byColor = colored.get(defaults)
  if (!byColor) colored.set(defaults, (byColor = new Map()))
  let m = byColor.get(to)
  if (!m) {
    m = new Map([...defaults].map(([n, v]) => [n, v.replaceAll(from, to)]))
    byColor.set(to, m)
  }
  return m
}

/** 一个元素要留下的声明：own 是它的全部声明，parent 是上级元素的全部声明（没有时为 null），defaults 是同名元素的默认值。 */
export function keepDeclarations(
  own: [string, string][],
  parent: Map<string, string> | null,
  defaults: Map<string, string>,
): [string, string][] {
  // 边框、轮廓等颜色的默认值是当前文字颜色：比较时把默认值中的默认文字颜色换成这个元素的文字颜色
  const color = own.find(([n]) => n === 'color')?.[1]
  const defaultColor = defaults.get('color')
  const isDefault = (name: string, value: string) => {
    const d = defaults.get(name)
    if (d === undefined) return false
    if (d === value) return true
    if (!color || !defaultColor || color === defaultColor || LITERAL_COLOR.has(name)) return false
    return withColor(defaults, defaultColor, color).get(name) === value
  }
  return own.filter(([name, value]) => {
    if (name.startsWith('--') || DROPPED.test(name)) return false
    // 会继承的属性（包括 color 本身）不做颜色替换：没有上级时只和默认值直接比
    if (inherited(name)) return parent ? parent.get(name) !== value : defaults.get(name) !== value
    return !isDefault(name, value)
  })
}

function slimSvg(svg: string): string {
  const doc = new DOMParser().parseFromString(svg, 'image/svg+xml')
  // 默认值从空白页中的同名元素取，写法和 html-to-image 一样（经 style 属性规整）
  const frame = document.createElement('iframe')
  frame.style.display = 'none'
  document.body.appendChild(frame)
  try {
    const fdoc = frame.contentDocument!
    const cache = new Map<string, Map<string, string>>()
    const defaults = (el: Element) => {
      const ns = el.namespaceURI ?? 'http://www.w3.org/1999/xhtml'
      const key = `${ns} ${el.localName}`
      let m = cache.get(key)
      if (m) return m
      const e = fdoc.createElementNS(ns, el.localName)
      const host = ns === SVG_NS && el.localName !== 'svg' ? fdoc.createElementNS(SVG_NS, 'svg') : null
      if (host) host.appendChild(e)
      fdoc.body.appendChild(host ?? e)
      const cs = frame.contentWindow!.getComputedStyle(e)
      const probe = document.createElement('div')
      for (const name of Array.from(cs)) {
        let v = cs.getPropertyValue(name)
        // html-to-image 把像素字号减了 0.1
        if (name === 'font-size' && v.endsWith('px')) v = `${Math.floor(parseFloat(v)) - 0.1}px`
        probe.style.setProperty(name, v)
      }
      ;(host ?? e).remove()
      m = new Map(splitStyle(probe.style.cssText))
      cache.set(key, m)
      return m
    }
    const styled = Array.from(doc.querySelectorAll('[style]'))
    const original = new Map(styled.map((el) => [el, new Map(splitStyle(el.getAttribute('style') ?? ''))]))
    for (const el of styled) {
      let p = el.parentElement
      while (p && !original.has(p)) p = p.parentElement
      const kept = keepDeclarations([...original.get(el)!], p ? original.get(p)! : null, defaults(el))
      el.setAttribute('style', kept.map(([n, v]) => `${n}: ${v}`).join('; '))
    }
    return new XMLSerializer().serializeToString(doc)
  } finally {
    frame.remove()
  }
}

const SVG_NS = 'http://www.w3.org/2000/svg'

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
