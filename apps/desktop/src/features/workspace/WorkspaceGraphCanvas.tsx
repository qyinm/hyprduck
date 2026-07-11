import {
  type Dispatch,
  type PointerEvent,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { Simulation } from "d3-force";

import { cn } from "@/lib/utils";
import { Focus, Network } from "lucide-react";

import {
  buildSigmaGraph,
  type BuildSigmaGraphSelection,
  type BuildSigmaGraphScope,
  type SigmaWorkspaceGraph,
} from "./graphologyAdapter";
import {
  buildForceSimulation,
  finiteGraphPosition,
  forceNodesToPositions,
  graphLayoutKey,
  graphPositionBounds,
  seedForceNodes,
  type ForceLink,
  type ForceNode,
} from "./graphForceLayout";
import { computeVisibleLabels, toPercentX, toPercentY } from "./graphLabelLayout";
import {
  graphPositionStorageKey,
  persistGraphPositions,
  readStoredGraphLayout,
} from "./graphPositionStorage";
import {
  fitGraphViewportToBounds,
  graphPositionFromPointerDelta,
  pointerDeltaToViewBox,
  zoomGraphViewportAtPoint,
} from "./graphViewport";
import type { WorkspaceUiAction, WorkspaceUiState } from "./state";
import type { WorkspaceProject } from "./types";

interface WorkspaceGraphCanvasProps {
  project: WorkspaceProject;
  uiState: WorkspaceUiState;
  dispatch: Dispatch<WorkspaceUiAction>;
  className?: string;
}

export function WorkspaceGraphCanvas(props: WorkspaceGraphCanvasProps) {
  const { project, uiState, dispatch, className } = props;
  const [graphMode, setGraphMode] =
    useState<BuildSigmaGraphScope["mode"]>("global");
  const graphScope = useMemo<BuildSigmaGraphScope>(
    () => sigmaGraphScopeFromUiState(graphMode, uiState),
    [graphMode, uiState.selectedNodeId],
  );
  const graphTopologySelection = sigmaGraphTopologySelectionFromUiState(uiState);
  const graphPresentationSelection = useMemo(
    () => sigmaGraphSelectionFromUiState(uiState),
    [uiState.selectedEdgeId, uiState.selectedNodeId],
  );
  const graph = useMemo(
    () => buildSigmaGraph(project, graphTopologySelection, graphScope),
    [graphScope, graphTopologySelection, project],
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
        selectedEdgeId={graphPresentationSelection.selectedEdgeId}
        selectedNodeId={graphPresentationSelection.selectedNodeId}
      />
    </div>
  );
}

const EMPTY_SIGMA_GRAPH_SELECTION: BuildSigmaGraphSelection = Object.freeze({
  selectedNodeId: null,
  selectedEdgeId: null,
});

export function sigmaGraphScopeFromUiState(
  graphMode: BuildSigmaGraphScope["mode"],
  uiState: WorkspaceUiState,
): BuildSigmaGraphScope {
  return {
    mode: graphMode,
    centerNodeId: graphMode === "local" ? uiState.selectedNodeId : null,
    depth: 1,
  };
}

export function sigmaGraphTopologySelectionFromUiState(
  _uiState: WorkspaceUiState,
): BuildSigmaGraphSelection {
  return EMPTY_SIGMA_GRAPH_SELECTION;
}

export function sigmaGraphSelectionFromUiState(
  uiState: WorkspaceUiState,
): BuildSigmaGraphSelection {
  return {
    selectedNodeId: uiState.selectedNodeId,
    selectedEdgeId: uiState.selectedEdgeId,
  };
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
            isSourceDocumentEdge
              ? 0.96
              : crossCluster && !selected
                ? Math.max(0.68, Math.min(0.88, viewport.zoom))
                : 1;
          const baseStroke = isSourceDocumentEdge
            ? "#475569"
            : crossCluster
              ? "#64748b"
              : "#cbd5e1";
          const baseStrokeWidth = isSourceDocumentEdge
            ? 0.58
            : crossCluster
              ? 0.46
              : 0.34;

          return (
            <line
              key={edge}
              data-graph-selectable="true"
              className="cursor-pointer"
              onClick={() => selectEdge(edge)}
              opacity={dimmed ? Math.min(0.18, baseOpacity) : hovered ? 1 : baseOpacity}
              stroke={selected || hovered ? "#111111" : baseStroke}
              strokeLinecap="round"
              strokeWidth={selected ? 0.72 : hovered ? 0.64 : baseStrokeWidth}
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

function nodeGraphPosition(
  graph: SigmaWorkspaceGraph,
  nodeId: string,
): { x: number; y: number } {
  const node = graph.getNodeAttributes(nodeId);
  return { x: node.x, y: node.y };
}
