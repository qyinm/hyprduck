import {
  type Dispatch,
  type PointerEvent,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

import { cn } from "@/lib/utils";
import { Focus, Network } from "lucide-react";

import {
  buildSigmaGraph,
  type BuildSigmaGraphScope,
  type SigmaWorkspaceGraph,
} from "./graphologyAdapter";
import {
  graphPositionFromPointerDelta,
  pointerDeltaToViewBox,
  zoomGraphViewportAtPoint,
} from "./graphViewport";
import type { WorkspaceUiAction, WorkspaceUiState } from "./state";
import type { WorkspaceProject } from "./types";

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
  const graphScope = useMemo<BuildSigmaGraphScope>(
    () => ({
      mode: graphMode,
      centerNodeId: uiState.selectedNodeId,
      depth: 1,
    }),
    [graphMode, uiState.selectedNodeId],
  );
  const graph = useMemo(
    () =>
      buildSigmaGraph(
        project,
        {
          selectedNodeId: uiState.selectedNodeId,
          selectedEdgeId: uiState.selectedEdgeId,
        },
        graphScope,
      ),
    [graphScope, project, uiState.selectedEdgeId, uiState.selectedNodeId],
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
}

function SvgGraphLayer(props: SvgGraphLayerProps) {
  const { graph, selectedEdgeId, selectedNodeId, dispatch } = props;
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
  const [nodePositions, setNodePositions] = useState<
    Record<string, { x: number; y: number }>
  >({});

  useEffect(() => {
    setNodePositions((current) => {
      const next = Object.fromEntries(
        Object.entries(current).filter(([nodeId]) => graph.hasNode(nodeId)),
      );
      return Object.keys(next).length === Object.keys(current).length
        ? current
        : next;
    });
  }, [graph]);

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
        return {
          ...current,
          [nodeDrag.nodeId]: {
            x: clampGraphPosition(base.x + graphDelta.x),
            y: clampGraphPosition(base.y + graphDelta.y),
          },
        };
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
      nodeDragRef.current = null;
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

          return (
            <line
              key={edge}
              data-graph-selectable="true"
              className="cursor-pointer"
              onClick={() => selectEdge(edge)}
              stroke={selected ? "#111111" : "#cbd5e1"}
              strokeDasharray={
                graph.getEdgeAttribute(edge, "edgeKind") === "source_document"
                  ? "1.2 1.4"
                  : undefined
              }
              strokeLinecap="round"
              strokeWidth={selected ? 0.55 : 0.34}
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
              <circle
                fill={selected ? "#111111" : "#ffffff"}
                r={selected ? 1.9 : data.nodeKind === "document" ? 1.65 : 1.35}
                stroke={selected ? "#111111" : "#111111"}
                strokeWidth={selected ? 0.28 : 0.2}
                vectorEffect="non-scaling-stroke"
              />
              <text
                dominantBaseline="middle"
                fill="#111111"
                fontFamily="Geist Variable, Geist, ui-sans-serif, system-ui"
                fontSize="1.55"
                fontWeight="500"
                x="2.6"
                y="0"
              >
                {data.label}
              </text>
            </g>
          );
        })}
      </g>
    </svg>
  );
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

function clampGraphPosition(value: number): number {
  return Math.max(-1.8, Math.min(1.8, value));
}
