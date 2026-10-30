// ER 图的连线：折线，同一张表、同一侧出来的线各走各的通道（`lane`），不叠在一起。

import { BaseEdge, type EdgeProps, Position, getSmoothStepPath } from '@xyflow/react'
import { memo } from 'react'

const LANE_GAP = 14
const CARD_OFFSET = 16

function ErEdgeLine({
  sourceX,
  sourceY,
  targetX,
  targetY,
  sourcePosition,
  targetPosition,
  markerEnd,
  label,
  data,
}: EdgeProps) {
  const { lane = 0, cards } = (data ?? {}) as { lane?: number; cards?: [string, string] }
  const [path, labelX, labelY] = getSmoothStepPath({
    sourceX,
    sourceY,
    targetX,
    targetY,
    sourcePosition,
    targetPosition,
    borderRadius: 6,
    offset: 18 + Math.abs(lane) * LANE_GAP,
    centerX: (sourceX + targetX) / 2 + lane * LANE_GAP,
  })
  const near = (x: number, y: number, side: Position, text: string) => (
    <g className="er-card" transform={`translate(${x + (side === Position.Left ? -CARD_OFFSET : CARD_OFFSET)}, ${y - 11})`}>
      <rect x={-8} y={-7} width={16} height={14} rx={3} />
      <text textAnchor="middle" dominantBaseline="central">
        {text}
      </text>
    </g>
  )
  return (
    <>
      <BaseEdge path={path} markerEnd={markerEnd} label={label} labelX={labelX} labelY={labelY} />
      {cards && near(sourceX, sourceY, sourcePosition, cards[0])}
      {cards && near(targetX, targetY, targetPosition, cards[1])}
    </>
  )
}

export default memo(ErEdgeLine)
