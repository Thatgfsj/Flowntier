/**
 * PlanGraph — React Flow visualization of a workflow plan DAG.
 *
 * Renders task nodes colored by role with status borders, and edges
 * showing hard (solid) or soft (dashed) dependencies.
 *
 * See `docs/TASK_GRAPH.md` §6 and `docs/UI_GUIDELINES.md`.
 */

import { useCallback, useMemo } from 'react';
import {
  ReactFlow,
  Background,
  Controls,
  MiniMap,
  type Node,
  type Edge,
  type NodeTypes,
  MarkerType,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';

// ── Types ────────────────────────────────────────────────────────

export type TaskStatus =
  | 'PENDING'
  | 'DISPATCHED'
  | 'IN_PROGRESS'
  | 'SUBMITTED'
  | 'UNDER_REVIEW'
  | 'REPAIR_REQUESTED'
  | 'REPAIRING'
  | 'APPROVED'
  | 'DONE'
  | 'FAILED'
  | 'ABORTED';

export type EdgeKind = 'Hard' | 'Soft';

export interface PlanTaskNode {
  id: string;
  title: string;
  owner_role: string;
  depends_on: string[];
  status: TaskStatus;
  est_tokens?: number;
}

export interface PlanEdge {
  from: string;
  to: string;
  kind: EdgeKind;
}

export interface PlanGraphProps {
  nodes: PlanTaskNode[];
  edges: PlanEdge[];
  onNodeClick?: (nodeId: string) => void;
  className?: string;
}

// ── Role colors ──────────────────────────────────────────────────

const ROLE_COLORS: Record<string, string> = {
  backend: '#14b8a6',
  frontend: '#f59e0b',
  database: '#8b5cf6',
  api: '#3b82f6',
  qa: '#ef4444',
  docs: '#6b7280',
  devops: '#10b981',
  default: '#64748b',
};

const STATUS_BORDER: Record<TaskStatus, string> = {
  PENDING: '#94a3b8',
  DISPATCHED: '#3b82f6',
  IN_PROGRESS: '#3b82f6',
  SUBMITTED: '#8b5cf6',
  UNDER_REVIEW: '#f59e0b',
  REPAIR_REQUESTED: '#ef4444',
  REPAIRING: '#f59e0b',
  APPROVED: '#22c55e',
  DONE: '#22c55e',
  FAILED: '#ef4444',
  ABORTED: '#94a3b8',
};

const STATUS_LABEL: Record<TaskStatus, string> = {
  PENDING: '待办',
  DISPATCHED: '已派发',
  IN_PROGRESS: '进行中',
  SUBMITTED: '已提交',
  UNDER_REVIEW: '评审中',
  REPAIR_REQUESTED: '需修复',
  REPAIRING: '修复中',
  APPROVED: '已通过',
  DONE: '完成',
  FAILED: '失败',
  ABORTED: '已中止',
};

// ── Custom node component ────────────────────────────────────────

function TaskNodeComponent({ data }: { data: Record<string, unknown> }) {
  const role = (data.owner_role as string) ?? 'default';
  const status = (data.status as TaskStatus) ?? 'PENDING';
  const bgColor = ROLE_COLORS[role] ?? ROLE_COLORS.default;
  const borderColor = STATUS_BORDER[status];
  const isRunning = status === 'IN_PROGRESS' || status === 'REPAIRING';

  return (
    <div
      className={`rounded-md px-3 py-2 text-xs font-medium shadow-sm transition-all ${
        isRunning ? 'animate-pulse' : ''
      }`}
      style={{
        backgroundColor: `${bgColor}20`,
        borderLeft: `3px solid ${bgColor}`,
        border: `1px solid ${borderColor}`,
        minWidth: 140,
      }}
    >
      <div className="truncate font-semibold text-primary">
        {data.title as string}
      </div>
      <div className="mt-1 flex items-center justify-between gap-2">
        <span className="text-text-secondary">{role}</span>
        <span
          className="rounded-full px-1.5 py-0.5 text-[10px] uppercase"
          style={{ color: borderColor, backgroundColor: `${borderColor}20` }}
        >
          {STATUS_LABEL[status]}
        </span>
      </div>
      {data.est_tokens != null && (
        <div className="mt-1 text-[10px] text-text-secondary">
          ~{String(data.est_tokens)} tokens
        </div>
      )}
    </div>
  );
}

const nodeTypes: NodeTypes = {
  task: TaskNodeComponent,
};

// ── Layout algorithm (simple topological layers) ─────────────────

function layoutNodes(tasks: PlanTaskNode[]): Node[] {
  // Compute topological layers
  const layers = new Map<string, number>();
  const visited = new Set<string>();

  function getLayer(id: string): number {
    if (layers.has(id)) return layers.get(id)!;
    if (visited.has(id)) return 0; // cycle protection
    visited.add(id);

    const task = tasks.find((t) => t.id === id);
    if (!task || task.depends_on.length === 0) {
      layers.set(id, 0);
      return 0;
    }

    const maxDep = Math.max(...task.depends_on.map(getLayer));
    const layer = maxDep + 1;
    layers.set(id, layer);
    return layer;
  }

  tasks.forEach((t) => getLayer(t.id));

  // Group by layer
  const layerGroups = new Map<number, PlanTaskNode[]>();
  tasks.forEach((t) => {
    const layer = layers.get(t.id) ?? 0;
    if (!layerGroups.has(layer)) layerGroups.set(layer, []);
    layerGroups.get(layer)!.push(t);
  });

  // Position nodes
  const NODE_HEIGHT = 80;
  const LAYER_GAP = 200;
  const NODE_GAP = 120;

  const nodes: Node[] = [];
  layerGroups.forEach((group, layerIdx) => {
    const totalHeight = group.length * NODE_HEIGHT + (group.length - 1) * NODE_GAP;
    const startY = -totalHeight / 2;

    group.forEach((task, nodeIdx) => {
      nodes.push({
        id: task.id,
        type: 'task',
        position: {
          x: layerIdx * LAYER_GAP,
          y: startY + nodeIdx * (NODE_HEIGHT + NODE_GAP),
        },
        data: {
          title: task.title,
          owner_role: task.owner_role,
          status: task.status,
          est_tokens: task.est_tokens,
        },
      });
    });
  });

  return nodes;
}

// ── Main component ───────────────────────────────────────────────

export function PlanGraph({ nodes, edges, onNodeClick, className }: PlanGraphProps) {
  const flowNodes = useMemo(() => layoutNodes(nodes), [nodes]);

  const flowEdges: Edge[] = useMemo(
    () =>
      edges.map((e) => ({
        id: `${e.from}-${e.to}`,
        source: e.from,
        target: e.to,
        type: 'smoothstep',
        animated: e.kind === 'Soft',
        style: {
          stroke: e.kind === 'Hard' ? '#64748b' : '#94a3b8',
          strokeWidth: 1.5,
          strokeDasharray: e.kind === 'Soft' ? '5 5' : undefined,
        },
        markerEnd: {
          type: MarkerType.ArrowClosed,
          width: 16,
          height: 16,
          color: e.kind === 'Hard' ? '#64748b' : '#94a3b8',
        },
      })),
    [edges],
  );

  const handleNodeClick = useCallback(
    (_: React.MouseEvent, node: Node) => {
      onNodeClick?.(node.id);
    },
    [onNodeClick],
  );

  return (
    <div className={className ?? 'h-[400px] w-full rounded-lg border border-border'}>
      <ReactFlow
        nodes={flowNodes}
        edges={flowEdges}
        nodeTypes={nodeTypes}
        onNodeClick={handleNodeClick}
        fitView
        attributionPosition="bottom-left"
        proOptions={{ hideAttribution: true }}
      >
        <Background gap={16} size={1} />
        <Controls />
        <MiniMap
          nodeColor={(node: Node): string => {
            const role = (node.data?.owner_role as string) ?? 'default';
            return (ROLE_COLORS[role] ?? ROLE_COLORS.default) as string;
          }}
          maskColor="rgba(0,0,0,0.1)"
        />
      </ReactFlow>
    </div>
  );
}
