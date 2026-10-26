// 结果表格（M1 设计文档 2.1 节）：Glide Data Grid，canvas 渲染。行号、数值右对齐、NULL 灰显；
// 滚动到接近已加载的末尾时续取下一批；双击或回车查看单元格；选中区域可用 Ctrl+C 复制（制表符分隔）。

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
import { useMemo, useState } from 'react'

import { type ResultSet, cellText } from '../../shared/exec'
import { useSetting } from '../../shared/settings'
import { THEME_KEY } from '../../shared/theme'
import CellViewer, { type ViewedCell } from './CellViewer'
import { isNumeric } from './cell'
import { shouldFetchMore } from './results'

/** 按列名和前几行的内容估计列宽。 */
function columnWidth(r: ResultSet, col: number): number {
  let chars = r.columns[col].name.length + 2
  for (const row of r.rows.slice(0, 50)) chars = Math.max(chars, cellText(row[col]).length)
  return Math.min(400, Math.max(64, chars * 7 + 20))
}

/** 表格配色取自外壳的 CSS 变量，主题切换时重新读取。 */
function useGridTheme(): Partial<Theme> {
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
      baseFontStyle: '12px',
      headerFontStyle: '600 12px',
    }
  }, [pref, dark]) // 主题变化时重新读取
}

export interface ResultGridProps {
  result: ResultSet
  /** 服务端在等 fetch_more（这个结果集还有下一批）。 */
  waiting: boolean
  onFetchMore(): void
}

export default function ResultGrid({ result, waiting, onFetchMore }: ResultGridProps) {
  const theme = useGridTheme()
  const [viewed, setViewed] = useState<ViewedCell | null>(null)
  // 列宽在列信息和第一批行到达时估计，续取的行不影响
  const hasRows = result.rows.length > 0
  const columns: GridColumn[] = useMemo(
    () => result.columns.map((c, i) => ({ id: String(i), title: c.name, width: columnWidth(result, i) })),
    [result.columns, hasRows],
  )
  const muted = useMemo(() => ({ textDark: theme.textLight }), [theme])

  const getCellContent = ([col, row]: Item): GridCell => {
    const c = result.rows[row]?.[col] ?? { t: 'null' }
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
        onCellActivated={([col, row]) =>
          setViewed({ column: result.columns[col].name, row: row + 1, value: result.rows[row][col] })
        }
        rowMarkers="number"
        smoothScrollX
        smoothScrollY
        rowHeight={24}
        headerHeight={28}
        theme={theme}
        width="100%"
        height="100%"
      />
      <CellViewer cell={viewed} onClose={() => setViewed(null)} />
    </>
  )
}
