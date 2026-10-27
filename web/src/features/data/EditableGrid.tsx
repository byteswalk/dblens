// 数据标签的可编辑表格（M2a 设计文档第 6 节）：在已加载的行后面接新增的行；改过的单元格、新增行、删除行、
// 解析失败的单元格和提交失败的行用不同底色标出。双击或回车编辑，Ctrl+V 粘贴，右键菜单设为 NULL、撤销、删除行。

import '@glideapps/glide-data-grid/dist/index.css'

import {
  CompactSelection,
  DataEditor,
  type GridCell,
  GridCellKind,
  type GridColumn,
  type GridSelection,
  type Item,
  type Rectangle,
} from '@glideapps/glide-data-grid'
import { App, Dropdown } from 'antd'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { type CellValue, type ResultSet, cellText } from '../../shared/exec'
import { columnWidth, useGridTheme } from '../results/ResultGrid'
import { isNumeric } from '../results/cell'
import { shouldFetchMore } from '../results/results'
import { type EditState, type Pending, deleteRows, paste, setCell, undoCell, undoRow } from './edit'
import type { EditColumn } from './editable'
import { parseCell } from './parse'
import { type Order, sortedTitle } from './sort'

export interface EditableGridProps {
  result: ResultSet
  waiting: boolean
  onFetchMore(): void
  sort?: Order
  onSort?(column: string): void
  /** 结果集每一列能否编辑；整张表只读时 readOnly 为 true。 */
  columns: EditColumn[]
  readOnly: boolean
  edit: EditState
  onEdit(next: EditState): void
  /** 提交失败的行。 */
  failedRow: number | null
  /** 表格中选中的行（删除、复制行用）。 */
  onSelectRows(rows: number[]): void
}

const emptySelection: GridSelection = { columns: CompactSelection.empty(), rows: CompactSelection.empty() }

/** 大字段超过预览长度时只读：编辑会把截断的内容写回去。 */
function truncated(v: CellValue | undefined): boolean {
  return v?.t === 'lob' && (v.v as { truncated?: boolean } | undefined)?.truncated === true
}

export default function EditableGrid(props: EditableGridProps) {
  const { result, waiting, onFetchMore, sort, onSort, columns, readOnly, edit, onEdit, failedRow, onSelectRows } = props
  const { t } = useTranslation()
  const { message } = App.useApp()
  const theme = useGridTheme()
  const [selection, setSelection] = useState<GridSelection>(emptySelection)
  const [menu, setMenu] = useState<{ x: number; y: number; cell: Item } | null>(null)
  const loaded = result.rows.length
  const hasRows = loaded > 0
  const gridColumns: GridColumn[] = useMemo(
    () => result.columns.map((c, i) => ({ id: String(i), title: sortedTitle(c.name, sort), width: columnWidth(result, i) })),
    [result.columns, hasRows, sort],
  )
  const css = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim()
  const colors = useMemo(
    () => ({
      changed: { bgCell: css('--warn-bg') },
      inserted: { bgCell: css('--ok-bg') },
      error: { bgCell: css('--danger-bg'), textDark: css('--danger') },
      deleted: { bgCell: css('--hover'), textDark: css('--muted') },
      muted: { textDark: theme.textLight },
    }),
    [theme],
  )

  const original = (row: number, col: number): CellValue | undefined => (row < loaded ? result.rows[row][col] : undefined)
  const pendingOf = (row: number, col: number): Pending | undefined =>
    row < loaded ? edit.updates[row]?.[col] : edit.inserts[row - loaded]?.[col]
  const canEdit = (row: number, col: number) =>
    !readOnly && columns[col]?.editable && !edit.deletes.includes(row) && !truncated(original(row, col))

  const getCellContent = ([col, row]: Item): GridCell => {
    const pending = pendingOf(row, col)
    const value = pending && 'value' in pending ? pending.value : (original(row, col) ?? { t: 'null' })
    const text = pending && 'error' in pending ? pending.text : cellText(value)
    const isNull = !(pending && 'error' in pending) && value.t === 'null'
    const override =
      pending && 'error' in pending
        ? colors.error
        : edit.deletes.includes(row)
          ? colors.deleted
          : row === failedRow
            ? colors.error
            : row >= loaded
              ? colors.inserted
              : pending
                ? colors.changed
                : isNull
                  ? colors.muted
                  : undefined
    return {
      kind: GridCellKind.Text,
      // 编辑框里的初始文本：NULL 为空
      data: isNull ? '' : text,
      displayData: text.length > 500 ? `${text.slice(0, 500)}…` : text,
      allowOverlay: canEdit(row, col),
      readonly: !canEdit(row, col),
      contentAlign: isNumeric(value) ? 'right' : 'left',
      themeOverride: override,
    }
  }

  const parse = (col: number, text: string): Pending => {
    const p = parseCell(text, columns[col].logical)
    return 'value' in p ? { value: p.value } : { error: p.error, text }
  }

  const set = (row: number, col: number, cell: Pending) => onEdit(setCell(edit, loaded, row, col, cell, original(row, col)))

  const rowsOf = (sel: GridSelection, fallback: number) => {
    const rows = sel.rows.toArray()
    return rows.length ? rows : [fallback]
  }

  const menuItems = menu
    ? [
        { key: 'null', label: t('editor.data.edit.setNull'), disabled: !canEdit(menu.cell[1], menu.cell[0]) },
        { key: 'undoCell', label: t('editor.data.edit.undoCell'), disabled: !pendingOf(menu.cell[1], menu.cell[0]) },
        { key: 'undoRow', label: t('editor.data.edit.undoRow') },
        { type: 'divider' as const },
        { key: 'copy', label: t('editor.data.edit.copyCell') },
        { key: 'delete', label: t('editor.data.edit.deleteRows'), danger: true, disabled: readOnly },
      ]
    : []

  const onMenu = (key: string) => {
    if (!menu) return
    const [col, row] = menu.cell
    setMenu(null)
    if (key === 'null') set(row, col, { value: { t: 'null' } })
    else if (key === 'undoCell') onEdit(undoCell(edit, loaded, row, col))
    else if (key === 'undoRow') onEdit(undoRow(edit, loaded, row))
    else if (key === 'delete') onEdit(deleteRows(edit, loaded, rowsOf(selection, row)))
    else if (key === 'copy') {
      const cell = getCellContent([col, row])
      void navigator.clipboard.writeText(cell.kind === GridCellKind.Text ? cell.data : '')
    }
  }

  return (
    <>
      <DataEditor
        columns={gridColumns}
        rows={loaded + edit.inserts.length}
        getCellContent={getCellContent}
        getCellsForSelection
        onVisibleRegionChanged={(range: Rectangle) => {
          if (shouldFetchMore(range.y + range.height, loaded, waiting)) onFetchMore()
        }}
        onHeaderClicked={onSort ? (col) => onSort(result.columns[col].name) : undefined}
        onCellEdited={([col, row], cell) => {
          if (cell.kind !== GridCellKind.Text || !canEdit(row, col)) return
          const before = getCellContent([col, row])
          // 打开编辑框后没改就关上：不算改动（NULL 的编辑框是空的，不能当成空字符串写回去）
          if (before.kind === GridCellKind.Text && before.data === cell.data) return
          set(row, col, parse(col, cell.data))
        }}
        onPaste={([col, row], values) => {
          if (readOnly) return false
          const r = paste(
            edit,
            loaded,
            result.rows,
            { row, col },
            values.map((v) => [...v]),
            columns.map((c, i) => ({ editable: c.editable, parse: (text: string) => parse(i, text) })),
          )
          onEdit(r.state)
          if (r.ignoredColumns) message.warning(t('editor.data.edit.ignoredColumns', { count: r.ignoredColumns }))
          return false
        }}
        // Delete 键不清空单元格（删除行用工具栏或右键菜单）
        onDelete={() => false}
        gridSelection={selection}
        onGridSelectionChange={(sel) => {
          setSelection(sel)
          onSelectRows(rowsOf(sel, sel.current?.cell[1] ?? -1).filter((r) => r >= 0))
        }}
        onCellContextMenu={(cell, e) => {
          e.preventDefault()
          setMenu({ x: e.bounds.x + e.localEventX, y: e.bounds.y + e.localEventY, cell })
        }}
        rowMarkers="both"
        smoothScrollX
        smoothScrollY
        rowHeight={28}
        headerHeight={32}
        theme={theme}
        width="100%"
        height="100%"
      />
      <Dropdown
        open={menu !== null}
        onOpenChange={(open) => !open && setMenu(null)}
        menu={{ items: menuItems, onClick: ({ key }) => onMenu(key) }}
        trigger={['contextMenu']}
      >
        <span style={{ position: 'fixed', left: menu?.x ?? 0, top: menu?.y ?? 0, width: 1, height: 1 }} />
      </Dropdown>
    </>
  )
}
