import {
  type Dispatch,
  type PointerEvent,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

import { cn } from "@/lib/utils";

import {
  buildSigmaGraph,
  type SigmaWorkspaceGraph,
} from "./graphologyAdapter";
import { pointerDeltaToViewBox, zoomGraphViewportAtPoint } from "./graphViewport";
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
  const graph = useMemo(
    () =>
      buildSigmaGraph(project, {
        selectedNodeId: uiState.selectedNodeId,
        selectedEdgeId: uiState.selectedEdgeId,
      }),
    [project, uiState.selectedEdgeId, uiState.selectedNodeId],
  );

  return (
    <div className={cn("relative size-full overflow-hidden bg-white", className)}>
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
  const suppressClickRef = useRef(false);
  const [viewport, setViewport] = useState({
    panX: 0,
    panY: 0,
    zoom: 1,
  });

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

    event.currentTarget.setPointerCapture(event.pointerId);
    dragRef.current = {
      pointerId: event.pointerId,
      lastX: event.clientX,
      lastY: event.clientY,
      totalDelta: 0,
    };
  };

  const handlePointerMove = (event: PointerEvent<SVGSVGElement>) => {
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
      panX: current.panX + pointerDeltaToViewBox(deltaX, rect.width, current.zoom),
      panY: current.panY + pointerDeltaToViewBox(deltaY, rect.height, current.zoom),
    }));
  };

  const handlePointerUp = (event: PointerEvent<SVGSVGElement>) => {
    if (dragRef.current?.pointerId === event.pointerId) {
      dragRef.current = null;
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
      <g transform={`translate(${viewport.panX} ${viewport.panY}) scale(${viewport.zoom})`}>
        {edges.map((edge) => {
          const source = graph.source(edge);
          const target = graph.target(edge);
          const sourceNode = graph.getNodeAttributes(source);
          const targetNode = graph.getNodeAttributes(target);
          const selected = selectedEdgeId === edge;

          return (
            <line
              key={edge}
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
              x1={toPercentX(sourceNode.x)}
              x2={toPercentX(targetNode.x)}
              y1={toPercentY(sourceNode.y)}
              y2={toPercentY(targetNode.y)}
            />
          );
        })}

        {nodes.map((node) => {
          const data = graph.getNodeAttributes(node);
          const selected = selectedNodeId === node;

          return (
            <g
              key={node}
              className="cursor-pointer"
              onClick={() => selectNode(node)}
              transform={`translate(${toPercentX(data.x)} ${toPercentY(data.y)})`}
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
