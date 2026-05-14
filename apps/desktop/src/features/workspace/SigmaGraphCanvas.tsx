import {
  type Dispatch,
  type PointerEvent,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  forceCenter,
  forceCollide,
  forceLink,
  forceManyBody,
  forceSimulation,
  forceX,
  forceY,
  type Simulation,
  type SimulationLinkDatum,
  type SimulationNodeDatum,
} from "d3-force";

import { cn } from "@/lib/utils";
import { Focus, Network } from "lucide-react";

import {
  buildSigmaGraph,
  type BuildSigmaGraphScope,
  type SigmaNodeAttributes,
  type SigmaWorkspaceGraph,
} from "./graphologyAdapter";
import {
  fitGraphViewportToBounds,
  type GraphViewport,
  graphPositionFromPointerDelta,
  pointerDeltaToViewBox,
  zoomGraphViewportAtPoint,
} from "./graphViewport";
import type { WorkspaceUiAction, WorkspaceUiState } from "./state";
import type { WorkspaceNodeKind, WorkspaceProject } from "./types";

interface SigmaGraphCanvasProps {
  project: WorkspaceProject;
  uiState: WorkspaceUiState;
  dispatch: Dispatch<WorkspaceUiAction>;
  className?: string;
}

export function SigmaGraphCanvas(props: SigmaGraphCanvasProps) {
  const { project, uiState, dispatch, className } = props;
  const [graphMode, setGraphMode] =
    useState<BuildSigmaGraphScope["mode"]>("global");
  const localCenterNodeId =
    graphMode === "local" ? uiState.selectedNodeId : null;
  const graphScope = useMemo<BuildSigmaGraphScope>(
    () => ({
      mode: graphMode,
      centerNodeId: localCenterNodeId,
      depth: 1,
    }),
    [graphMode, localCenterNodeId],
  );
  const graph = useMemo(
    () =>
      buildSigmaGraph(
        project,
        {
          selectedNodeId: null,
          selectedEdgeId: null,
        },
        graphScope,
      ),
    [graphScope, project],
  );
  const localModeDisabled = !uiState.selectedNodeId;

  return (
    <div className={cn("relative size-full overflow-hidden bg-white", className)}>
      <div className="absolute right-4 top-4 z-20 flex items-center gap-1 rounded-full border border-border/80 bg-background/90 p-1 shadow-sm backdrop-blur">
        <button
          aria-label="Show global graph"
          className={cn(
            "inline-flex size-8 items-center justify-center rounded-full text-muted-foreground transition hover:bg-secondary hover:text-foreground",
            graphMode === "global" &&
              "bg-foreground text-background hover:bg-foreground hover:text-background",
          )}
          onClick={() => setGraphMode("global")}
          title="Global graph"
          type="button"
        >
          <Network size={15} />
        </button>
        <button
          aria-label="Show local graph"
          className={cn(
            "inline-flex size-8 items-center justify-center rounded-full text-muted-foreground transition hover:bg-secondary hover:text-foreground disabled:pointer-events-none disabled:opacity-40",
            graphMode === "local" &&
              "bg-foreground text-background hover:bg-foreground hover:text-background",
          )}
          disabled={localModeDisabled}
          onClick={() => setGraphMode("local")}
          title="Local graph"
          type="button"
        >
          <Focus size={15} />
        </button>
      </div>
      <SvgGraphLayer
        dispatch={dispatch}
        graph={graph}
        positionStorageKey={graphPositionStorageKey(
          project.summary.projectId,
          graphScope,
        )}
        selectedEdgeId={uiState.selectedEdgeId}
        selectedNodeId={uiState.selectedNodeId}
      />
    </div>
  );
}

interface SvgGraphLayerProps {
  graph: SigmaWorkspaceGraph;
  selectedNodeId: string | null;
  selectedEdgeId: string | null;
  dispatch: Dispatch<WorkspaceUiAction>;
  positionStorageKey: string;
}

function SvgGraphLayer(props: SvgGraphLayerProps) {
  const { graph, selectedEdgeId, selectedNodeId, dispatch, positionStorageKey } =
    props;
  const nodes = graph.nodes();
  const edges = graph.edges();
  const svgRef = useRef<SVGSVGElement | null>(null);
  const dragRef = useRef<{
    pointerId: number;
    lastX: number;
    lastY: number;
    totalDelta: number;
  } | null>(null);
  const nodeDragRef = useRef<{
    pointerId: number;
    nodeId: string;
    lastX: number;
    lastY: number;
    totalDelta: number;
  } | null>(null);
  const suppressClickRef = useRef(false);
  const [viewport, setViewport] = useState({
    panX: 0,
    panY: 0,
    zoom: 1,
  });
  const [hoveredNodeId, setHoveredNodeId] = useState<string | null>(null);
  const [nodePositions, setNodePositions] = useState<
    Record<string, { x: number; y: number }>
  >({});
  const nodePositionsRef = useRef(nodePositions);
  const pinnedNodeIdsRef = useRef<Set<string>>(new Set());
  const simulationRef = useRef<Simulation<ForceNode, ForceLink> | null>(null);
  const simulationNodesRef = useRef<Map<string, ForceNode>>(new Map());
  const frameRef = useRef<number | null>(null);
  const layoutKey = useMemo(() => graphLayoutKey(graph), [graph]);
  const activeHoveredNodeId =
    hoveredNodeId !== null && graph.hasNode(hoveredNodeId)
      ? hoveredNodeId
      : null;
  const visibleLabelNodeIds = useMemo(
    () =>
      computeVisibleLabels({
        graph,
        nodes,
        positions: nodePositions,
        viewport,
        selectedNodeId,
        hoveredNodeId: activeHoveredNodeId,
      }),
    [activeHoveredNodeId, graph, nodePositions, nodes, selectedNodeId, viewport],
  );

  useEffect(() => {
    const saved = readStoredGraphLayout(positionStorageKey);
    pinnedNodeIdsRef.current = new Set(
      Object.keys(saved.nodes).filter((nodeId) => graph.hasNode(nodeId)),
    );
    const seeded = seedForceNodes(graph, saved.nodes, nodePositionsRef.current);
    simulationNodesRef.current = new Map(seeded.nodes.map((node) => [node.id, node]));
    nodePositionsRef.current = forceNodesToPositions(seeded.nodes);
    setNodePositions(nodePositionsRef.current);

    simulationRef.current?.stop();
    const simulation = buildForceSimulation(
      seeded.nodes,
      seeded.links,
    );
    simulation.stop();
    const preTicks = seeded.nodes.length > 800 ? 50 : 100;
    for (let index = 0; index < preTicks; index += 1) {
      simulation.tick();
    }
    const initialPositions = forceNodesToPositions(seeded.nodes);
    nodePositionsRef.current = initialPositions;
    setNodePositions(initialPositions);
    const bounds = graphPositionBounds(initialPositions);
    if (bounds) {
      setViewport(fitGraphViewportToBounds(bounds));
    }
    simulation.alpha(0.25).restart();
    simulationRef.current = simulation;
    simulation.on("tick", () => {
      if (frameRef.current !== null) {
        return;
      }

      frameRef.current = window.requestAnimationFrame(() => {
        frameRef.current = null;
        const next = forceNodesToPositions(seeded.nodes);
        nodePositionsRef.current = next;
        setNodePositions(next);
      });
    });

    return () => {
      simulation.stop();
      if (frameRef.current !== null) {
        window.cancelAnimationFrame(frameRef.current);
        frameRef.current = null;
      }
      if (simulationRef.current === simulation) {
        simulationRef.current = null;
      }
    };
  }, [graph, layoutKey, positionStorageKey]);

  useEffect(() => {
    const svg = svgRef.current;
    if (!svg) {
      return;
    }

    const handleNativeWheel = (event: WheelEvent) => {
      event.preventDefault();
      const rect = svg.getBoundingClientRect();
      const point = {
        x: ((event.clientX - rect.left) / rect.width) * 100,
        y: ((event.clientY - rect.top) / rect.height) * 100,
      };

      setViewport((current) =>
        zoomGraphViewportAtPoint(current, event.deltaY, point),
      );
    };

    svg.addEventListener("wheel", handleNativeWheel, { passive: false });
    return () => {
      svg.removeEventListener("wheel", handleNativeWheel);
    };
  }, []);

  const handlePointerDown = (event: PointerEvent<SVGSVGElement>) => {
    if (event.button !== 0) {
      return;
    }
    const target = event.target;
    if (
      target instanceof Element &&
      target.closest("[data-graph-selectable='true']")
    ) {
      return;
    }

    event.currentTarget.setPointerCapture(event.pointerId);
    dragRef.current = {
      pointerId: event.pointerId,
      lastX: event.clientX,
      lastY: event.clientY,
      totalDelta: 0,
    };
  };

  const handlePointerMove = (event: PointerEvent<SVGSVGElement>) => {
    const nodeDrag = nodeDragRef.current;
    if (nodeDrag?.pointerId === event.pointerId) {
      const svg = svgRef.current;
      if (!svg) {
        return;
      }

      const rect = svg.getBoundingClientRect();
      const deltaX = event.clientX - nodeDrag.lastX;
      const deltaY = event.clientY - nodeDrag.lastY;
      const graphDelta = graphPositionFromPointerDelta(
        deltaX,
        deltaY,
        rect.width,
        rect.height,
        viewport.zoom,
      );
      const nextDelta =
        nodeDrag.totalDelta + Math.abs(deltaX) + Math.abs(deltaY);

      nodeDragRef.current = {
        ...nodeDrag,
        lastX: event.clientX,
        lastY: event.clientY,
        totalDelta: nextDelta,
      };

      if (nextDelta > 2) {
        suppressClickRef.current = true;
      }

      setNodePositions((current) => {
        const base =
          current[nodeDrag.nodeId] ?? nodeGraphPosition(graph, nodeDrag.nodeId);
        const position = {
          x: finiteGraphPosition(base.x + graphDelta.x),
          y: finiteGraphPosition(base.y + graphDelta.y),
        };
        const forceNode = simulationNodesRef.current.get(nodeDrag.nodeId);
        if (forceNode) {
          forceNode.x = position.x;
          forceNode.y = position.y;
          forceNode.fx = position.x;
          forceNode.fy = position.y;
        }
        simulationRef.current?.alphaTarget(0.18).restart();
        const next = {
          ...current,
          [nodeDrag.nodeId]: position,
        };
        nodePositionsRef.current = next;
        return next;
      });
      return;
    }

    const drag = dragRef.current;
    const svg = svgRef.current;
    if (!drag || drag.pointerId !== event.pointerId || !svg) {
      return;
    }

    const rect = svg.getBoundingClientRect();
    const deltaX = event.clientX - drag.lastX;
    const deltaY = event.clientY - drag.lastY;
    const nextDelta = drag.totalDelta + Math.abs(deltaX) + Math.abs(deltaY);
    dragRef.current = {
      ...drag,
      lastX: event.clientX,
      lastY: event.clientY,
      totalDelta: nextDelta,
    };

    if (nextDelta > 3) {
      suppressClickRef.current = true;
    }

    setViewport((current) => ({
      ...current,
      panX:
        current.panX + pointerDeltaToViewBox(deltaX, rect.width, current.zoom),
      panY:
        current.panY + pointerDeltaToViewBox(deltaY, rect.height, current.zoom),
    }));
  };

  const handlePointerUp = (event: PointerEvent<SVGSVGElement>) => {
    if (nodeDragRef.current?.pointerId === event.pointerId) {
      const forceNode = simulationNodesRef.current.get(nodeDragRef.current.nodeId);
      const persisted = nodePositionsRef.current[nodeDragRef.current.nodeId];
      if (forceNode && persisted) {
        forceNode.fx = persisted.x;
        forceNode.fy = persisted.y;
      }
      pinnedNodeIdsRef.current.add(nodeDragRef.current.nodeId);
      simulationRef.current?.alphaTarget(0);
      nodeDragRef.current = null;
      persistGraphPositions(
        positionStorageKey,
        nodePositionsRef.current,
        pinnedNodeIdsRef.current,
      );
    }

    if (dragRef.current?.pointerId === event.pointerId) {
      dragRef.current = null;
    }

    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }

    if (suppressClickRef.current) {
      window.setTimeout(() => {
        suppressClickRef.current = false;
      }, 0);
    }
  };

  const selectNode = (node: string) => {
    if (suppressClickRef.current) {
      suppressClickRef.current = false;
      return;
    }

    dispatch({ type: "select_node", nodeId: node });
  };

  const selectEdge = (edge: string) => {
    if (suppressClickRef.current) {
      suppressClickRef.current = false;
      return;
    }

    dispatch({ type: "select_edge", edgeId: edge });
  };

  const handleNodePointerDown = (
    event: PointerEvent<SVGGElement>,
    nodeId: string,
  ) => {
    if (event.button !== 0) {
      return;
    }

    event.preventDefault();
    event.stopPropagation();
    svgRef.current?.setPointerCapture(event.pointerId);
    nodeDragRef.current = {
      pointerId: event.pointerId,
      nodeId,
      lastX: event.clientX,
      lastY: event.clientY,
      totalDelta: 0,
    };
    const position = nodePositionsRef.current[nodeId] ?? nodeGraphPosition(graph, nodeId);
    const forceNode = simulationNodesRef.current.get(nodeId);
    if (forceNode) {
      forceNode.fx = position.x;
      forceNode.fy = position.y;
    }
    simulationRef.current?.alphaTarget(0.18).restart();
    dispatch({ type: "select_node", nodeId });
  };

  return (
    <svg
      ref={svgRef}
      aria-label="Knowledge graph"
      className="absolute inset-0 z-10 size-full cursor-grab touch-none select-none bg-white active:cursor-grabbing"
      onPointerCancel={handlePointerUp}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerUp}
      role="img"
      viewBox="0 0 100 100"
    >
      <g
        transform={`translate(${viewport.panX} ${viewport.panY}) scale(${viewport.zoom})`}
      >
        {edges.map((edge) => {
          const source = graph.source(edge);
          const target = graph.target(edge);
          const sourceNode = graph.getNodeAttributes(source);
          const targetNode = graph.getNodeAttributes(target);
          const selected = selectedEdgeId === edge;
          const edgeKind = graph.getEdgeAttribute(edge, "edgeKind");
          const isSourceDocumentEdge = edgeKind === "source_document";
          const crossCluster = sourceNode.clusterId !== targetNode.clusterId;
          const hovered =
            activeHoveredNodeId !== null &&
            (source === activeHoveredNodeId || target === activeHoveredNodeId);
          const dimmed = activeHoveredNodeId !== null && !hovered && !selected;
          const baseOpacity =
            crossCluster && !selected
              ? isSourceDocumentEdge
                ? Math.max(0.72, Math.min(1, viewport.zoom))
                : Math.min(0.35, viewport.zoom)
              : 1;

          return (
            <line
              key={edge}
              data-graph-selectable="true"
              className="cursor-pointer"
              onClick={() => selectEdge(edge)}
              opacity={dimmed ? Math.min(0.18, baseOpacity) : hovered ? 1 : baseOpacity}
              stroke={selected || hovered ? "#111111" : isSourceDocumentEdge ? "#94a3b8" : "#cbd5e1"}
              strokeDasharray={isSourceDocumentEdge ? "1.2 1.4" : undefined}
              strokeLinecap="round"
              strokeWidth={selected ? 0.62 : hovered ? 0.5 : isSourceDocumentEdge ? 0.42 : 0.34}
              vectorEffect="non-scaling-stroke"
              x1={toPercentX(nodePositions[source]?.x ?? sourceNode.x)}
              x2={toPercentX(nodePositions[target]?.x ?? targetNode.x)}
              y1={toPercentY(nodePositions[source]?.y ?? sourceNode.y)}
              y2={toPercentY(nodePositions[target]?.y ?? targetNode.y)}
            />
          );
        })}

        {nodes.map((node) => {
          const data = graph.getNodeAttributes(node);
          const selected = selectedNodeId === node;
          const position = nodePositions[node] ?? data;

          return (
            <g
              key={node}
              data-graph-selectable="true"
              className="cursor-grab active:cursor-grabbing"
              onClick={() => selectNode(node)}
              onPointerDown={(event) => handleNodePointerDown(event, node)}
              transform={`translate(${toPercentX(position.x)} ${toPercentY(position.y)})`}
            >
              <title>{data.label}</title>
              <circle
                fill={selected ? "#111111" : "#ffffff"}
                onPointerEnter={() => setHoveredNodeId(node)}
                onPointerLeave={() =>
                  setHoveredNodeId((current) => (current === node ? null : current))
                }
                r={selected ? 1.9 : data.nodeKind === "document" ? 1.65 : 1.35}
                stroke={selected ? "#111111" : "#111111"}
                strokeWidth={selected ? 0.28 : 0.2}
                vectorEffect="non-scaling-stroke"
              />
              {visibleLabelNodeIds.has(node) && (
                <text
                  dominantBaseline="hanging"
                  fill="#111111"
                  fontFamily="Geist Variable, Geist, ui-sans-serif, system-ui"
                  fontSize={2.15 / viewport.zoom}
                  fontWeight={selected ? "650" : "500"}
                  paintOrder="stroke"
                  stroke="#ffffff"
                  strokeWidth={0.65 / viewport.zoom}
                  textAnchor="middle"
                  x="0"
                  y={3.2 / viewport.zoom}
                  pointerEvents="none"
                >
                  {data.shortLabel}
                </text>
              )}
            </g>
          );
        })}
      </g>
    </svg>
  );
}

function graphLayoutKey(graph: SigmaWorkspaceGraph): string {
  return JSON.stringify({
    nodes: graph.nodes().sort(),
    edges: graph
      .edges()
      .map((edge) => [
        graph.source(edge),
        graph.target(edge),
        graph.getEdgeAttribute(edge, "edgeKind"),
      ])
      .sort((a, b) => a.join(":").localeCompare(b.join(":"))),
  });
}

function graphPositionBounds(
  positions: Record<string, { x: number; y: number }>,
): { minX: number; minY: number; maxX: number; maxY: number } | null {
  const values = Object.values(positions).filter(
    (position) => Number.isFinite(position.x) && Number.isFinite(position.y),
  );
  if (values.length === 0) {
    return null;
  }

  return {
    minX: Math.min(...values.map((position) => position.x)),
    minY: Math.min(...values.map((position) => position.y)),
    maxX: Math.max(...values.map((position) => position.x)),
    maxY: Math.max(...values.map((position) => position.y)),
  };
}

function computeVisibleLabels(params: {
  graph: SigmaWorkspaceGraph;
  nodes: string[];
  positions: Record<string, { x: number; y: number }>;
  viewport: GraphViewport;
  selectedNodeId: string | null;
  hoveredNodeId: string | null;
}): Set<string> {
  const { graph, nodes, positions, viewport, selectedNodeId, hoveredNodeId } =
    params;
  const occupied: Array<{ x1: number; y1: number; x2: number; y2: number }> = [];
  const visible = new Set<string>();
  const candidates = nodes
    .map((nodeId) => {
      const data = graph.getNodeAttributes(nodeId);
      const selected = selectedNodeId === nodeId;
      const hovered = hoveredNodeId === nodeId;
      if (!labelEligible(data, viewport.zoom, selected, hovered)) {
        return null;
      }
      return {
        nodeId,
        data,
        position: positions[nodeId] ?? { x: data.x, y: data.y },
        selected,
        hovered,
        priority:
          data.labelPriority + (selected ? 10_000 : 0) + (hovered ? 9_000 : 0),
      };
    })
    .filter((candidate): candidate is NonNullable<typeof candidate> => Boolean(candidate))
    .sort((a, b) => b.priority - a.priority || a.nodeId.localeCompare(b.nodeId));

  for (const candidate of candidates) {
    const rect = estimatedLabelRect({
      label: candidate.data.shortLabel,
      position: candidate.position,
      viewport,
    });
    const forced = candidate.selected || candidate.hovered;
    if (!forced && occupied.some((other) => rectsOverlap(rect, other))) {
      continue;
    }
    visible.add(candidate.nodeId);
    occupied.push(rect);
  }

  return visible;
}

function labelEligible(
  data: SigmaNodeAttributes,
  zoom: number,
  selected: boolean,
  hovered: boolean,
): boolean {
  if (selected || hovered) {
    return true;
  }
  if (data.nodeKind === "source" || data.nodeKind === "document") {
    return zoom >= 0.18;
  }
  if (zoom < 0.75) {
    return false;
  }
  if (zoom < 1.15) {
    return data.relatedCount >= 4 || data.evidenceCount >= 3;
  }
  return true;
}

function estimatedLabelRect(params: {
  label: string;
  position: { x: number; y: number };
  viewport: GraphViewport;
}): { x1: number; y1: number; x2: number; y2: number } {
  const { label, position, viewport } = params;
  const x = toPercentX(position.x) * viewport.zoom + viewport.panX;
  const y = toPercentY(position.y) * viewport.zoom + viewport.panY;
  const fontSize = 2.15;
  const width = Math.max(4, displayCells(label) * fontSize * 0.42);
  const height = fontSize * 1.25;
  const gap = 2.8;

  return {
    x1: x - width / 2,
    x2: x + width / 2,
    y1: y + gap,
    y2: y + gap + height,
  };
}

function rectsOverlap(
  a: { x1: number; y1: number; x2: number; y2: number },
  b: { x1: number; y1: number; x2: number; y2: number },
): boolean {
  return a.x1 < b.x2 && a.x2 > b.x1 && a.y1 < b.y2 && a.y2 > b.y1;
}

function displayCells(value: string): number {
  let cells = 0;
  for (const char of Array.from(value)) {
    cells += /[\u1100-\u11FF\u2E80-\uA4CF\uAC00-\uD7A3\uF900-\uFAFF\uFE10-\uFE6F\uFF00-\uFF60\uFFE0-\uFFE6]/.test(
      char,
    )
      ? 2
      : 1;
  }
  return cells;
}

function graphPositionStorageKey(
  projectId: string,
  scope: BuildSigmaGraphScope,
): string {
  if (scope.mode === "local" && scope.centerNodeId) {
    return `hyprduck:graph-layout:v2:${projectId}:local:${scope.centerNodeId}`;
  }

  return `hyprduck:graph-layout:v2:${projectId}:global`;
}

interface ForceNode extends SimulationNodeDatum {
  id: string;
  x: number;
  y: number;
  radius: number;
  clusterId: string;
  nodeKind: WorkspaceNodeKind;
  anchorX: number;
  anchorY: number;
}

interface ForceLink extends SimulationLinkDatum<ForceNode> {
  source: string | ForceNode;
  target: string | ForceNode;
  edgeKind: string;
  sameCluster: boolean;
}

function seedForceNodes(
  graph: SigmaWorkspaceGraph,
  storedPositions: Record<string, { x: number; y: number }>,
  currentPositions: Record<string, { x: number; y: number }>,
): {
  nodes: ForceNode[];
  links: ForceLink[];
} {
  const nodes = graph.nodes().map((nodeId) => {
    const data = graph.getNodeAttributes(nodeId);
    const stored = storedPositions[nodeId];
    const current = currentPositions[nodeId];
    const start = stored ?? current ?? { x: data.x, y: data.y };
    return {
      id: nodeId,
      x: finiteGraphPosition(start.x),
      y: finiteGraphPosition(start.y),
      radius: data.nodeKind === "source" || data.nodeKind === "document" ? 0.105 : 0.08,
      clusterId: data.clusterId,
      nodeKind: data.nodeKind,
      anchorX: data.x,
      anchorY: data.y,
      fx: stored ? finiteGraphPosition(stored.x) : undefined,
      fy: stored ? finiteGraphPosition(stored.y) : undefined,
    };
  });
  const links = graph.edges().map((edgeId) => {
    const source = graph.source(edgeId);
    const target = graph.target(edgeId);
    return {
      source,
      target,
      edgeKind: graph.getEdgeAttribute(edgeId, "edgeKind"),
      sameCluster:
        graph.getNodeAttribute(source, "clusterId") ===
        graph.getNodeAttribute(target, "clusterId"),
    };
  });
  return { nodes, links };
}

function buildForceSimulation(
  nodes: ForceNode[],
  links: ForceLink[],
): Simulation<ForceNode, ForceLink> {
  return forceSimulation<ForceNode>(nodes)
    .alpha(0.85)
    .alphaMin(0.008)
    .alphaDecay(0.045)
    .velocityDecay(0.5)
    .force(
      "charge",
      forceManyBody<ForceNode>()
        .strength((node) =>
          node.nodeKind === "source" || node.nodeKind === "document"
            ? -0.34
            : -0.22,
        )
        .distanceMin(0.08)
        .distanceMax(2.4),
    )
    .force(
      "link",
      forceLink<ForceNode, ForceLink>(links)
        .id((node) => node.id)
        .distance((link) => {
          if (link.edgeKind === "source_document") {
            return 0.28;
          }
          return link.sameCluster ? 0.58 : 1.55;
        })
        .strength((link) => {
          if (link.edgeKind === "source_document") {
            return 0.75;
          }
          return link.sameCluster ? 0.045 : 0.006;
        }),
    )
    .force(
      "collide",
      forceCollide<ForceNode>()
        .radius((node) => node.radius + 0.055)
        .strength(0.85)
        .iterations(2),
    )
    .force("clusterX", forceX<ForceNode>((node) => node.anchorX).strength(0.075))
    .force("clusterY", forceY<ForceNode>((node) => node.anchorY).strength(0.075))
    .force("center", forceCenter<ForceNode>(0, 0).strength(0.012));
}

function forceNodesToPositions(nodes: ForceNode[]): Record<string, { x: number; y: number }> {
  return Object.fromEntries(
    nodes.map((node) => [
      node.id,
      {
        x: finiteGraphPosition(node.x),
        y: finiteGraphPosition(node.y),
      },
    ]),
  );
}

interface StoredGraphLayout {
  version: 2;
  nodes: Record<string, { x: number; y: number; pinned: true; updatedAt: number }>;
}

function readStoredGraphLayout(key: string): StoredGraphLayout {
  if (typeof window === "undefined") {
    return { version: 2, nodes: {} };
  }
  try {
    const raw = window.localStorage.getItem(key);
    if (!raw) {
      return { version: 2, nodes: {} };
    }
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object") {
      return { version: 2, nodes: {} };
    }

    const rawNodes =
      (parsed as { version?: unknown; nodes?: unknown }).version === 2 &&
      (parsed as { nodes?: unknown }).nodes &&
      typeof (parsed as { nodes?: unknown }).nodes === "object"
        ? ((parsed as { nodes: Record<string, unknown> }).nodes)
        : (parsed as Record<string, unknown>);
    const nodes: StoredGraphLayout["nodes"] = {};
    for (const [nodeId, value] of Object.entries(rawNodes)) {
      const rawX = (value as { x?: unknown } | null)?.x;
      const rawY = (value as { y?: unknown } | null)?.y;
      if (
        value &&
        typeof value === "object" &&
        typeof rawX === "number" &&
        typeof rawY === "number" &&
        Number.isFinite(rawX) &&
        Number.isFinite(rawY)
      ) {
        nodes[nodeId] = {
          x: finiteGraphPosition(rawX),
          y: finiteGraphPosition(rawY),
          pinned: true,
          updatedAt:
            typeof (value as { updatedAt?: unknown }).updatedAt === "number"
              ? (value as { updatedAt: number }).updatedAt
              : 0,
        };
      }
    }
    return { version: 2, nodes };
  } catch {
    return { version: 2, nodes: {} };
  }
}

function persistGraphPositions(
  key: string,
  positions: Record<string, { x: number; y: number }>,
  pinnedNodeIds: Set<string>,
) {
  if (typeof window === "undefined") {
    return;
  }
  try {
    const existing = readStoredGraphLayout(key);
    const next: StoredGraphLayout = {
      version: 2,
      nodes: { ...existing.nodes },
    };
    const updatedAt = Date.now();
    for (const nodeId of pinnedNodeIds) {
      const position = positions[nodeId];
      if (
        position &&
        Number.isFinite(position.x) &&
        Number.isFinite(position.y)
      ) {
        next.nodes[nodeId] = {
          x: position.x,
          y: position.y,
          pinned: true,
          updatedAt,
        };
      }
    }

    if (Object.keys(next.nodes).length === 0) {
      window.localStorage.removeItem(key);
    } else {
      window.localStorage.setItem(key, JSON.stringify(next));
    }
  } catch {
    // Local graph positioning should never block graph interaction.
  }
}

function toPercentX(value: number): number {
  return value * 50 + 50;
}

function toPercentY(value: number): number {
  return 50 - value * 50;
}

function nodeGraphPosition(
  graph: SigmaWorkspaceGraph,
  nodeId: string,
): { x: number; y: number } {
  const node = graph.getNodeAttributes(nodeId);
  return { x: node.x, y: node.y };
}

function finiteGraphPosition(value: number, fallback = 0): number {
  return Number.isFinite(value) ? value : fallback;
}
