// ER 图中的表节点（M4b 设计文档 4.2 节）：表头为表名，每列一行（主键、外键图标，列名，类型）。
// 每列左边是被引用的连接点（`列:in`），右边是引用别的表的连接点（`列:out`）；表头另有一对（只显示表名时用）。

import { IconKey, IconLink } from '@tabler/icons-react'
import { Handle, type Node, type NodeProps, Position } from '@xyflow/react'
import { memo } from 'react'
import { useTranslation } from 'react-i18next'

import { type ErNodeData, TABLE_HANDLE } from './graph'

export type TableNodeType = Node<ErNodeData, 'table'>

/** 连线走另一侧时用的连接点，只用于画线，不显示、不能拖。 */
function SideHandles({ id }: { id: string }) {
  return (
    <>
      <Handle type="source" position={Position.Left} id={`${id}:out:l`} className="er-handle side" isConnectable={false} />
      <Handle type="target" position={Position.Right} id={`${id}:in:r`} className="er-handle side" isConnectable={false} />
    </>
  )
}

function TableNode({ data, selected }: NodeProps<TableNodeType>) {
  const { t } = useTranslation()
  return (
    <div className={`er-table${selected ? ' selected' : ''}${data.error ? ' error' : ''}`} title={data.error ?? data.comment ?? undefined}>
      <div className="er-table-h">
        <Handle type="target" position={Position.Left} id={`${TABLE_HANDLE}:in`} className="er-handle" />
        <SideHandles id={TABLE_HANDLE} />
        <span className="er-table-name" title={data.comment ?? undefined}>
          {data.name}
          {data.showComments && data.comment && <span className="er-comment"> {data.comment}</span>}
        </span>
        {data.junction && (
          <span className="er-junction" title={`${data.junction[0]} ↔ ${data.junction[1]}`}>
            {t('er.relation.junction')}
          </span>
        )}
        <Handle type="source" position={Position.Right} id={`${TABLE_HANDLE}:out`} className="er-handle" />
      </div>
      {data.columns.map((c) => (
        <div key={c.name} className="er-col" title={[c.name, c.type, c.comment].filter(Boolean).join('  ')}>
          <Handle type="target" position={Position.Left} id={`${c.name}:in`} className="er-handle" />
          <SideHandles id={c.name} />
          <span className="er-col-icon">
            {c.pk ? <IconKey size={12} className="er-pk" /> : c.fk ? <IconLink size={12} className="er-fk" /> : null}
          </span>
          <span className={c.nullable ? 'er-col-name muted' : 'er-col-name'}>{c.name}</span>
          <span className="er-col-type">{data.showComments && c.comment ? c.comment : c.type}</span>
          <Handle type="source" position={Position.Right} id={`${c.name}:out`} className="er-handle" />
        </div>
      ))}
      {data.more > 0 && <div className="er-more">{t('er.relation.more', { count: data.more })}</div>}
      {data.external.map((x) => (
        <div key={x} className="er-external muted">
          {x}
        </div>
      ))}
    </div>
  )
}

export type FrameNodeType = Node<{ label: string; kind: 'page' | 'group' }, 'frame'>

/** 画布上的 A4 页面、业务主题边框：只是背景，不能选中和拖动。 */
export const FrameNode = memo(function FrameNode({ data }: NodeProps<FrameNodeType>) {
  return <div className={`er-frame ${data.kind}`}>{data.label}</div>
})

export default memo(TableNode)
