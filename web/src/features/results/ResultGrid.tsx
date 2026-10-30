// 结果表格（M1 设计文档 2.1 节）：Glide Data Grid，canvas 渲染。行号、数值右对齐、NULL 灰显；
// 滚动到接近已加载的末尾时续取下一批；双击或回车查看单元格；选中区域可用 Ctrl+C 复制（制表符分隔）。
// 数据标签（M1c2）：点列头排序，排序的列头带箭头。

import '@glideapps/glide-data-grid/dist/index.css'

import {
  DataEditor,
  type GridCell,
  GridCellKind,
  type GridColumn,
  type Item,
  type Rectangle,
  type Theme,
} from '@glideapps/glide-data-grid'
import { useEffect, useMemo, useState } from 'react'

import { type ResultSet, cellText } from '../../shared/exec'
import { type Order, nextOrder, sortedTitle } from '../data/sort'
import { useSetting } from '../../shared/settings'
import { THEME_KEY } from '../../shared/theme'
import CellViewer, { type ViewedCell } from './CellViewer'
import { isNumeric } from './cell'
import { shouldFetchMore } from './results'

/** 按列名和前几行的内容估计列宽。 */
export function columnWidth(r: ResultSet, col: number, sample = 50, max = 420): number {
  let chars = r.columns[col].name.length + 2
  for (const row of r.rows.slice(0, sample)) chars = Math.max(chars, cellText(row[col]).length)
  return Math.min(max, Math.max(72, chars * 8 + 24))
}

/** 已加载的行按 order 排序后的下标：数值按大小，其余按文字，NULL 排最后。 */
export function sortedRows(r: ResultSet, order: Order | undefined): number[] {
  const rows = r.rows.map((_, i) => i)
  const col = order ? r.columns.findIndex((c) => c.name === order.column) : -1
  if (!order || col < 0) return rows
  const sign = order.desc ? -1 : 1
  return rows.sort((a, b) => {
    const [x, y] = [r.rows[a][col], r.rows[b][col]]
    if (x.t === 'null' || y.t === 'null') return x.t === y.t ? a - b : x.t === 'null' ? 1 : -1
    const [tx, ty] = [cellText(x), cellText(y)]
    const d = isNumeric(x) && isNumeric(y) ? Number(tx) - Number(ty) : tx.localeCompare(ty, undefined, { numeric: true })
    return d * sign || a - b
  })
}

/** 表格配色取自外壳的 CSS 变量，主题切换时重新读取。 */
export function useGridTheme(): Partial<Theme> {
  const pref = useSetting(THEME_KEY)
  const dark = document.documentElement.dataset.theme
  return useMemo(() => {
    const css = getComputedStyle(document.documentElement)
    const v = (name: string) => css.getPropertyValue(name).trim()
    return {
      accentColor: v('--accent'),
      accentLight: v('--sel'),
      textDark: v('--text'),
      textMedium: v('--text-2'),
      textLight: v('--muted'),
      textHeader: v('--text-2'),
      bgCell: v('--panel'),
      bgCellMedium: v('--panel-2'),
      bgHeader: v('--panel-2'),
      bgHeaderHasFocus: v('--hover'),
      bgHeaderHovered: v('--hover'),
      borderColor: v('--border'),
      fontFamily: v('--mono'),
      baseFontStyle: '13px',
      headerFontStyle: '600 13px',
      markerFontStyle: '12px',
    }
  }, [pref, dark]) // 主题变化时重新读取
}

export interface ResultGridProps {
  result: ResultSet
  /** 服务端在等 fetch_more（这个结果集还有下一批）。 */
  waiting: boolean
  onFetchMore(): void
  /** 服务端排序（数据标签）；不给 onSort 时点列头只排已加载的行。 */
  sort?: Order
  onSort?(column: string): void
  /** 递增时按全部已加载的行自适应列宽。 */
  fitKey?: number
}

export default function ResultGrid({ result, waiting, onFetchMore, sort, onSort, fitKey = 0 }: ResultGridProps) {
  const theme = useGridTheme()
  const [viewed, setViewed] = useState<ViewedCell | null>(null)
  // 列宽在列信息和第一批行到达时估计，续取的行不影响
  const hasRows = result.rows.length > 0
  const [widths, setWidths] = useState<Record<number, number>>({})
  useEffect(() => {
    if (fitKey) setWidths(Object.fromEntries(result.columns.map((_, i) => [i, columnWidth(result, i, Infinity, 800)])))
    // 只在点按钮时重算
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fitKey])
  const [localSort, setLocalSort] = useState<Order>(null)
  const order = onSort ? sort : localSort
  const rowCount = result.rows.length
  const view = useMemo(() => sortedRows(result, onSort ? null : localSort), [result, rowCount, localSort, onSort])
  const columns: GridColumn[] = useMemo(
    () => result.columns.map((c, i) => ({ id: String(i), title: sortedTitle(c.name, order), width: widths[i] ?? columnWidth(result, i) })),
    [result.columns, hasRows, order, widths],
  )
  const muted = useMemo(() => ({ textDark: theme.textLight }), [theme])

  const getCellContent = ([col, row]: Item): GridCell => {
    const c = result.rows[view[row]]?.[col] ?? { t: 'null' }
    const text = cellText(c)
    return {
      kind: GridCellKind.Text,
      data: text,
      displayData: text.length > 500 ? `${text.slice(0, 500)}…` : text,
      allowOverlay: false,
      contentAlign: isNumeric(c) ? 'right' : 'left',
      themeOverride: c.t === 'null' ? muted : undefined,
    }
  }

  const onVisible = (range: Rectangle) => {
    if (shouldFetchMore(range.y + range.height, result.rows.length, waiting)) onFetchMore()
  }

  return (
    <>
      <DataEditor
        columns={columns}
        rows={result.rows.length}
        getCellContent={getCellContent}
        getCellsForSelection
        onVisibleRegionChanged={onVisible}
        onHeaderClicked={(col) => {
          const name = result.columns[col].name
          if (onSort) onSort(name)
          else setLocalSort((o) => nextOrder(o, name))
        }}
        onColumnResize={(_, width, col) => setWidths((w) => ({ ...w, [col]: width }))}
        onCellActivated={([col, row]) =>
          setViewed({ column: result.columns[col].name, row: view[row] + 1, value: result.rows[view[row]][col] })
        }
        rowMarkers="number"
        smoothScrollX
        smoothScrollY
        rowHeight={28}
        headerHeight={32}
        theme={theme}
        width="100%"
        height="100%"
      />
      <CellViewer cell={viewed} onClose={() => setViewed(null)} />
    </>
  )
}
