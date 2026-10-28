// ER 图中的表节点（M4b 设计文档 4.2 节）：表头为表名，每列一行（主键、外键图标，列名，类型）。
// 每列左边是被引用的连接点（`列:in`），右边是引用别的表的连接点（`列:out`）；表头另有一对（只显示表名时用）。

import { IconKey, IconLink } from '@tabler/icons-react'
import { Handle, type Node, type NodeProps, Position } from '@xyflow/react'
import { memo } from 'react'

import { type ErNodeData, TABLE_HANDLE } from './graph'

export type TableNodeType = Node<ErNodeData, 'table'>

function TableNode({ data, selected }: NodeProps<TableNodeType>) {
  return (
    <div className={`er-table${selected ? ' selected' : ''}${data.error ? ' error' : ''}`} title={data.error ?? data.comment ?? undefined}>
      <div className="er-table-h">
        <Handle type="target" position={Position.Left} id={`${TABLE_HANDLE}:in`} className="er-handle" />
        <span className="er-table-name">{data.name}</span>
        <Handle type="source" position={Position.Right} id={`${TABLE_HANDLE}:out`} className="er-handle" />
      </div>
      {data.columns.map((c) => (
        <div key={c.name} className="er-col">
          <Handle type="target" position={Position.Left} id={`${c.name}:in`} className="er-handle" />
          <span className="er-col-icon">
            {c.pk ? <IconKey size={12} className="er-pk" /> : c.fk ? <IconLink size={12} className="er-fk" /> : null}
          </span>
          <span className={c.nullable ? 'er-col-name muted' : 'er-col-name'}>{c.name}</span>
          <span className="er-col-type">{c.type}</span>
          <Handle type="source" position={Position.Right} id={`${c.name}:out`} className="er-handle" />
        </div>
      ))}
      {data.external.map((x) => (
        <div key={x} className="er-external muted">
          {x}
        </div>
      ))}
    </div>
  )
}

export default memo(TableNode)
