// ER 图的连线：折线，同一张表、同一侧出来的线各走各的通道（`lane`），不叠在一起。

import { BaseEdge, type EdgeProps, getSmoothStepPath } from '@xyflow/react'
import { memo } from 'react'

const LANE_GAP = 14

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
  const lane = (data as { lane?: number } | undefined)?.lane ?? 0
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
  return <BaseEdge path={path} markerEnd={markerEnd} label={label} labelX={labelX} labelY={labelY} />
}

export default memo(ErEdgeLine)
