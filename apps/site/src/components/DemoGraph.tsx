import { useEffect, useRef, useState } from "react";
import type { Simulation, SimulationNodeDatum, SimulationLinkDatum } from "d3-force";
import {
  forceCenter,
  forceCollide,
  forceLink,
  forceManyBody,
  forceSimulation,
  forceX,
  forceY,
} from "d3-force";

type DemoNodeKind = "source" | "document" | "concept";

interface DemoNode {
  id: string;
  label: string;
  kind: DemoNodeKind;
  evidenceCount: number;
  relatedCount: number;
  anchorX: number;
  anchorY: number;
}

interface DemoEdge {
  id: string;
  source: string;
  target: string;
  kind: string;
}

const SAMPLE_NODES: DemoNode[] = [
  { id: "src-board", label: "board-report.pdf", kind: "source", evidenceCount: 8, relatedCount: 3, anchorX: -0.42, anchorY: -0.42 },
  { id: "concept-revenue", label: "Q3 Revenue", kind: "concept", evidenceCount: 4, relatedCount: 4, anchorX: 0.08, anchorY: -0.66 },
  { id: "concept-growth", label: "Growth 72%", kind: "concept", evidenceCount: 3, relatedCount: 4, anchorX: 0.56, anchorY: -0.52 },
  { id: "concept-enterprise", label: "Enterprise", kind: "concept", evidenceCount: 3, relatedCount: 4, anchorX: -0.04, anchorY: 0.04 },
  { id: "concept-retention", label: "Retention 87%", kind: "concept", evidenceCount: 3, relatedCount: 4, anchorX: 0.46, anchorY: 0.16 },
  { id: "concept-segment", label: "Segment SaaS", kind: "concept", evidenceCount: 2, relatedCount: 3, anchorX: 0.86, anchorY: -0.12 },
  { id: "concept-arr", label: "ARR Forecast", kind: "concept", evidenceCount: 2, relatedCount: 2, anchorX: -0.48, anchorY: 0.20 },
  { id: "concept-new-arr", label: "Net New ARR", kind: "concept", evidenceCount: 1, relatedCount: 2, anchorX: 0.04, anchorY: 0.60 },
  { id: "concept-margin", label: "Gross Margin 92%", kind: "concept", evidenceCount: 1, relatedCount: 3, anchorX: 0.60, anchorY: 0.44 },
];

const SAMPLE_EDGES: DemoEdge[] = [
  { id: "e-src-revenue", source: "src-board", target: "concept-revenue", kind: "source_document" },
  { id: "e-src-enterprise", source: "src-board", target: "concept-enterprise", kind: "source_document" },
  { id: "e-revenue-growth", source: "concept-revenue", target: "concept-growth", kind: "related_to" },
  { id: "e-revenue-enterprise", source: "concept-revenue", target: "concept-enterprise", kind: "related_to" },
  { id: "e-growth-segment", source: "concept-growth", target: "concept-segment", kind: "related_to" },
  { id: "e-enterprise-retention", source: "concept-enterprise", target: "concept-retention", kind: "related_to" },
  { id: "e-enterprise-arr", source: "concept-enterprise", target: "concept-arr", kind: "related_to" },
  { id: "e-retention-margin", source: "concept-retention", target: "concept-margin", kind: "related_to" },
  { id: "e-arr-new-arr", source: "concept-arr", target: "concept-new-arr", kind: "related_to" },
  { id: "e-segment-margin", source: "concept-segment", target: "concept-margin", kind: "related_to" },
  { id: "e-revenue-retention", source: "concept-revenue", target: "concept-retention", kind: "related_to" },
  { id: "e-growth-retention", source: "concept-growth", target: "concept-retention", kind: "related_to" },
];

interface ForceNode extends SimulationNodeDatum {
  id: string;
  radius: number;
  anchorX: number;
  anchorY: number;
  nodeKind: DemoNodeKind;
}

interface ForceLink extends SimulationLinkDatum<ForceNode> {
  source: string | ForceNode;
  target: string | ForceNode;
  edgeKind: string;
  sameCluster: boolean;
}

interface Viewport {
  panX: number;
  panY: number;
  zoom: number;
}

function toPercentX(v: number): number {
  return v * 50 + 50;
}

function toPercentY(v: number): number {
  return 50 - v * 50;
}

function nextZoom(current: number, wheelDeltaY: number): number {
  const clamped = Math.max(-120, Math.min(120, wheelDeltaY));
  return Math.min(6, Math.max(0.05, current * Math.exp(-clamped * 0.0025)));
}

function zoomAtPoint(
  current: Viewport,
  wheelDeltaY: number,
  px: number,
  py: number,
): Viewport {
  const z = nextZoom(current.zoom, wheelDeltaY);
  if (z === current.zoom) return current;
  const gx = (px - current.panX) / current.zoom;
  const gy = (py - current.panY) / current.zoom;
  return { panX: px - gx * z, panY: py - gy * z, zoom: z };
}

function pointerToViewBox(delta: number, size: number, zoom: number): number {
  if (size <= 0 || zoom <= 0) return 0;
  return (delta / size) * (100 / zoom);
}

type Selection = { kind: "node"; id: string } | { kind: "edge"; id: string } | null;

interface DemoGraphProps {
  selected: Selection;
  onSelect: (sel: Selection) => void;
}

export function DemoGraph({ selected, onSelect }: DemoGraphProps) {
  const svgRef = useRef<SVGSVGElement | null>(null);
  const simRef = useRef<Simulation<ForceNode, ForceLink> | null>(null);
  const [viewport, setViewport] = useState<Viewport>({ panX: 0, panY: 0, zoom: 1 });
  const [positions, setPositions] = useState<Record<string, { x: number; y: number }>>({});
  const [hoveredNodeId, setHoveredNodeId] = useState<string | null>(null);
  const dragRef = useRef<{ pointerId: number; lastX: number; lastY: number; totalDelta: number } | null>(null);
  const nodeDragRef = useRef<{ pointerId: number; nodeId: string; lastX: number; lastY: number; totalDelta: number } | null>(null);
  const suppressClickRef = useRef(false);
  const [graphMode, setGraphMode] = useState<"global" | "local">("global");

  useEffect(() => {
    const nodeMap = new Map(SAMPLE_NODES.map((n) => [n.id, n]));
    const nodes: ForceNode[] = SAMPLE_NODES.map((n) => ({
      id: n.id,
      x: n.anchorX + (Math.random() - 0.5) * 0.08,
      y: n.anchorY + (Math.random() - 0.5) * 0.08,
      radius: n.kind === "source" || n.kind === "document" ? 0.105 : 0.08,
      anchorX: n.anchorX,
      anchorY: n.anchorY,
      nodeKind: n.kind,
    }));

    const links: ForceLink[] = SAMPLE_EDGES.map((e) => ({
      source: e.source,
      target: e.target,
      edgeKind: e.kind,
      sameCluster: false,
    }));

    const sim = forceSimulation<ForceNode>(nodes)
      .alpha(0.85)
      .alphaMin(0.008)
      .alphaDecay(0.045)
      .velocityDecay(0.5)
      .force("charge", forceManyBody<ForceNode>()
        .strength((d) => d.nodeKind === "source" || d.nodeKind === "document" ? -0.34 : -0.22)
        .distanceMin(0.08)
        .distanceMax(2.4))
      .force("link", forceLink<ForceNode, ForceLink>(links)
        .id((d) => d.id)
        .distance((l) => l.edgeKind === "source_document" ? 0.28 : 0.58)
        .strength((l) => l.edgeKind === "source_document" ? 0.75 : 0.045))
      .force("collide", forceCollide<ForceNode>()
        .radius((d) => d.radius + 0.055)
        .strength(0.85)
        .iterations(2))
      .force("clusterX", forceX<ForceNode>((d) => d.anchorX).strength(0.075))
      .force("clusterY", forceY<ForceNode>((d) => d.anchorY).strength(0.075))
      .force("center", forceCenter<ForceNode>(0, 0).strength(0.012));

    for (let i = 0; i < 60; i++) sim.tick();

    const initial: Record<string, { x: number; y: number }> = {};
    for (const n of nodes) initial[n.id] = { x: n.x, y: n.y };
    setPositions(initial);

    sim.on("tick", () => {
      const next: Record<string, { x: number; y: number }> = {};
      for (const n of nodes) next[n.id] = { x: n.x, y: n.y };
      setPositions(next);
    });

    sim.alpha(0.25).restart();
    simRef.current = sim;

    return () => { sim.stop(); };
  }, []);

  const handleWheel = (event: React.WheelEvent<SVGSVGElement>) => {
    event.preventDefault();
    const rect = svgRef.current?.getBoundingClientRect();
    if (!rect) return;
    const px = ((event.clientX - rect.left) / rect.width) * 100;
    const py = ((event.clientY - rect.top) / rect.height) * 100;
    setViewport((v) => zoomAtPoint(v, event.deltaY, px, py));
  };

  const handlePointerDown = (event: React.PointerEvent<SVGSVGElement>) => {
    if (event.button !== 0) return;
    const target = event.target as Element;
    if (target.closest("[data-node]") || target.closest("[data-edge]")) return;
    (event.currentTarget as SVGSVGElement).setPointerCapture(event.pointerId);
    dragRef.current = { pointerId: event.pointerId, lastX: event.clientX, lastY: event.clientY, totalDelta: 0 };
  };

  const handlePointerMove = (event: React.PointerEvent<SVGSVGElement>) => {
    const nd = nodeDragRef.current;
    if (nd && nd.pointerId === event.pointerId) {
      const delta = Math.abs(event.clientX - nd.lastX) + Math.abs(event.clientY - nd.lastY);
      nodeDragRef.current = { ...nd, lastX: event.clientX, lastY: event.clientY, totalDelta: nd.totalDelta + delta };
      if (nd.totalDelta + delta > 2) suppressClickRef.current = true;

      const rect = svgRef.current?.getBoundingClientRect();
      if (!rect) return;
      const dx = pointerToViewBox(event.clientX - nd.lastX, rect.width, viewport.zoom) / 50;
      const dy = -pointerToViewBox(event.clientY - nd.lastY, rect.height, viewport.zoom) / 50;

      setPositions((prev) => {
        const pos = prev[nd.nodeId] ?? SAMPLE_NODES.find((n) => n.id === nd.nodeId) ?? { x: 0, y: 0 };
        return { ...prev, [nd.nodeId]: { x: pos.x + dx, y: pos.y + dy } };
      });
      return;
    }

    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    const rect = svgRef.current?.getBoundingClientRect();
    if (!rect) return;
    const delta = Math.abs(event.clientX - drag.lastX) + Math.abs(event.clientY - drag.lastY);
    dragRef.current = { ...drag, lastX: event.clientX, lastY: event.clientY, totalDelta: drag.totalDelta + delta };
    if (delta > 3) suppressClickRef.current = true;
    setViewport((v) => ({
      ...v,
      panX: v.panX + pointerToViewBox(event.clientX - drag.lastX, rect.width, v.zoom),
      panY: v.panY + pointerToViewBox(event.clientY - drag.lastY, rect.height, v.zoom),
    }));
  };

  const handlePointerUp = (event: React.PointerEvent<SVGSVGElement>) => {
    if (nodeDragRef.current?.pointerId === event.pointerId) nodeDragRef.current = null;
    if (dragRef.current?.pointerId === event.pointerId) dragRef.current = null;
    if (suppressClickRef.current) {
      setTimeout(() => { suppressClickRef.current = false; }, 0);
    }
  };

  const handleNodePointerDown = (event: React.PointerEvent, nodeId: string) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    (svgRef.current as unknown as SVGSVGElement)?.setPointerCapture(event.pointerId);
    nodeDragRef.current = { pointerId: event.pointerId, nodeId, lastX: event.clientX, lastY: event.clientY, totalDelta: 0 };
    onSelect({ kind: "node", id: nodeId });
  };

  const selectedNodeId = selected?.kind === "node" ? selected.id : null;
  const selectedEdgeId = selected?.kind === "edge" ? selected.id : null;

  const nodeData = SAMPLE_NODES.map((n) => {
    const pos = positions[n.id] ?? { x: n.anchorX, y: n.anchorY };
    const selected = selectedNodeId === n.id;
    const hovered = hoveredNodeId === n.id;
    const radius = n.kind === "source" || n.kind === "document" ? 1.65 : 1.35;
    const size = selected ? radius + 0.3 : radius;
    return { ...n, pos, selected, hovered, size };
  });

  const edgeData = SAMPLE_EDGES.map((e) => {
    const selected = selectedEdgeId === e.id;
    const hovered = hoveredNodeId !== null && (
      e.source === hoveredNodeId || e.target === hoveredNodeId
    );
    return { ...e, selected, hovered };
  });

  const labelIds = new Set<string>();
  for (const n of nodeData) {
    if (n.selected || n.hovered || n.kind === "source" || n.kind === "document" || viewport.zoom >= 0.75) {
      labelIds.add(n.id);
    }
  }

  const nodeById = Object.fromEntries(SAMPLE_NODES.map((n) => [n.id, n]));

  const selectedNodeInfo = selected?.kind === "node" ? nodeById[selected.id] : null;
  const selectedEdgeInfo = selected?.kind === "edge" ? SAMPLE_EDGES.find((e) => e.id === selected.id) : null;
  const selectedNodeByName = selectedNodeInfo
    ? SAMPLE_NODES.find((n) => n.id === selectedNodeInfo.id)
    : null;

  return (
    <div className="graph-root">
      {/* Mode toggle */}
      <div className="graph-mode-toggle">
        <span
          className={"mode-btn" + (graphMode === "global" ? " mode-btn-active" : "")}
          onClick={() => setGraphMode("global")}
          title="Global graph"
        >
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <rect x="16" y="16" width="6" height="6" rx="1"/><rect x="2" y="16" width="6" height="6" rx="1"/><rect x="9" y="2" width="6" height="6" rx="1"/><path d="M5 16v-3a1 1 0 0 1 1-1h12a1 1 0 0 1 1 1v3"/><path d="M12 12V8"/>
          </svg>
        </span>
        <span
          className={"mode-btn" + (graphMode === "local" ? " mode-btn-active" : "")}
          onClick={() => setGraphMode("local")}
          title="Local graph"
        >
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="12" cy="12" r="3"/><path d="M3 7V5a2 2 0 0 1 2-2h2"/><path d="M17 3h2a2 2 0 0 1 2 2v2"/><path d="M21 17v2a2 2 0 0 1-2 2h-2"/><path d="M7 21H5a2 2 0 0 1-2-2v-2"/>
          </svg>
        </span>
      </div>

      {/* SVG graph */}
      <svg
        ref={svgRef}
        className="graph-svg"
        viewBox="0 0 100 100"
        preserveAspectRatio="xMidYMid meet"
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
        onPointerCancel={handlePointerUp}
        onWheel={handleWheel}
        style={{ cursor: dragRef.current ? "grabbing" : "grab" }}
      >
        <defs>
          <filter id="shadow">
            <feDropShadow dx="0" dy="0" stdDeviation="0.3" flood-color="#fff" flood-opacity="1"/>
          </filter>
        </defs>
        <g transform={`translate(${viewport.panX} ${viewport.panY}) scale(${viewport.zoom})`}>
          {/* Edges */}
          {edgeData.map((e) => {
            const src = SAMPLE_NODES.find((n) => n.id === e.source);
            const tgt = SAMPLE_NODES.find((n) => n.id === e.target);
            if (!src || !tgt) return null;
            const sp = positions[e.source] ?? { x: src.anchorX, y: src.anchorY };
            const tp = positions[e.target] ?? { x: tgt.anchorX, y: tgt.anchorY };
            const dimmed = hoveredNodeId !== null && !e.hovered && !e.selected;
            return (
              <line
                key={e.id}
                data-edge={e.id}
                className="graph-edge-line"
                x1={toPercentX(sp.x)} y1={toPercentY(sp.y)}
                x2={toPercentX(tp.x)} y2={toPercentY(tp.y)}
                stroke={e.selected || e.hovered ? "#111111" : "#cbd5e1"}
                strokeWidth={e.selected ? 0.62 : e.hovered ? 0.5 : 0.34}
                strokeLinecap="round"
                vectorEffect="non-scaling-stroke"
                opacity={dimmed ? 0.18 : 1}
                onClick={(event) => { event.stopPropagation(); onSelect({ kind: "edge", id: e.id }); }}
              />
            );
          })}

          {/* Nodes */}
          {nodeData.map((n) => (
            <g
              key={n.id}
              data-node={n.id}
              transform={`translate(${toPercentX(n.pos.x)} ${toPercentY(n.pos.y)})`}
              onPointerDown={(event) => handleNodePointerDown(event, n.id)}
              onPointerEnter={() => setHoveredNodeId(n.id)}
              onPointerLeave={() => setHoveredNodeId((c) => c === n.id ? null : c)}
              style={{ cursor: "grab" }}
            >
              <title>{n.label}</title>
              <circle
                r={n.size}
                fill={n.selected ? "#111111" : "#ffffff"}
                stroke="#111111"
                strokeWidth={n.selected ? 0.28 : 0.2}
                vectorEffect="non-scaling-stroke"
              />
              {labelIds.has(n.id) && (
                <text
                  dominantBaseline="hanging"
                  fill="#111111"
                  fontFamily="Geist Variable, Geist, ui-sans-serif, system-ui"
                  fontSize={2.15 / viewport.zoom}
                  fontWeight={n.selected ? "650" : "500"}
                  paintOrder="stroke"
                  stroke="#ffffff"
                  strokeWidth={0.65 / viewport.zoom}
                  textAnchor="middle"
                  x={0}
                  y={3.2 / viewport.zoom}
                  pointerEvents="none"
                >
                  {n.label}
                </text>
              )}
            </g>
          ))}
        </g>
      </svg>
    </div>
  );
}
