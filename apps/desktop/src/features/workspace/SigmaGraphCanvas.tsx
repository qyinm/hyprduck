import { type Dispatch, useEffect, useMemo, useRef } from "react";
import Sigma from "sigma";

import { cn } from "@/lib/utils";

import {
  buildSigmaGraph,
  type SigmaEdgeAttributes,
  type SigmaNodeAttributes,
  type SigmaWorkspaceGraph,
} from "./graphologyAdapter";
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
  const containerRef = useRef<HTMLDivElement | null>(null);
  const rendererRef = useRef<Sigma<SigmaNodeAttributes, SigmaEdgeAttributes> | null>(
    null,
  );
  const dispatchRef = useRef(dispatch);
  const graph = useMemo(
    () =>
      buildSigmaGraph(project, {
        selectedNodeId: uiState.selectedNodeId,
        selectedEdgeId: uiState.selectedEdgeId,
      }),
    [project, uiState.selectedEdgeId, uiState.selectedNodeId],
  );

  useEffect(() => {
    dispatchRef.current = dispatch;
  }, [dispatch]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) {
      return;
    }

    const renderer = new Sigma<SigmaNodeAttributes, SigmaEdgeAttributes>(
      graph,
      container,
      {
        allowInvalidContainer: true,
        autoCenter: true,
        autoRescale: true,
        enableEdgeEvents: true,
        hideEdgesOnMove: false,
        hideLabelsOnMove: true,
        itemSizesReference: "positions",
        labelColor: { color: "#111111" },
        labelFont: "Geist Variable, Geist, ui-sans-serif, system-ui",
        labelRenderedSizeThreshold: 8,
        labelSize: 13,
        labelWeight: "500",
        maxCameraRatio: 4,
        minCameraRatio: 0.18,
        renderEdgeLabels: false,
        renderLabels: true,
        stagePadding: 24,
        zIndex: true,
        nodeReducer: (_node, data) => ({
          color: data.color,
          forceLabel: data.forceLabel,
          highlighted: data.highlighted,
          label: data.label,
          size: data.size,
          x: data.x,
          y: data.y,
          zIndex: data.zIndex,
        }),
        edgeReducer: (_edge, data) => ({
          color: data.color,
          hidden: data.hidden,
          label: data.label,
          size: data.size,
          zIndex: data.selected ? 10 : 0,
        }),
      },
    );

    renderer.on("clickNode", ({ node }) => {
      dispatchRef.current({ type: "select_node", nodeId: node });
    });
    renderer.on("clickEdge", ({ edge }) => {
      dispatchRef.current({ type: "select_edge", edgeId: edge });
    });
    renderer.on("beforeClear", () => {
      applySigmaCanvasBackground(renderer, "#ffffff");
    });

    applySigmaCanvasBackground(renderer, "#ffffff");
    rendererRef.current = renderer;

    return () => {
      renderer.kill();
      rendererRef.current = null;
    };
  }, []);

  useEffect(() => {
    const renderer = rendererRef.current;
    if (!renderer) {
      return;
    }

    renderer.setGraph(graph);
    renderer.refresh();
  }, [graph]);

  return (
    <div className={cn("relative size-full overflow-hidden bg-white", className)}>
      <div ref={containerRef} className="absolute inset-0 bg-white opacity-0" />
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

  return (
    <svg
      aria-label="Knowledge graph"
      className="absolute inset-0 z-10 size-full bg-white"
      role="img"
      viewBox="0 0 100 100"
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
            className="cursor-pointer"
            onClick={() => dispatch({ type: "select_edge", edgeId: edge })}
            stroke={selected ? "#111111" : "#cbd5e1"}
            strokeDasharray={
              graph.getEdgeAttribute(edge, "edgeKind") === "source_document"
                ? "1.2 1.4"
                : undefined
            }
            strokeLinecap="round"
            strokeWidth={selected ? 0.55 : 0.34}
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
            onClick={() => dispatch({ type: "select_node", nodeId: node })}
            transform={`translate(${toPercentX(data.x)} ${toPercentY(data.y)})`}
          >
            <circle
              fill={selected ? "#111111" : "#ffffff"}
              r={selected ? 1.9 : data.nodeKind === "document" ? 1.65 : 1.35}
              stroke={selected ? "#111111" : "#111111"}
              strokeWidth={selected ? 0.28 : 0.2}
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
    </svg>
  );
}

function toPercentX(value: number): number {
  return value * 50 + 50;
}

function toPercentY(value: number): number {
  return 50 - value * 50;
}

function applySigmaCanvasBackground(
  renderer: Sigma<SigmaNodeAttributes, SigmaEdgeAttributes>,
  backgroundColor: string,
) {
  const layerHost = renderer.getContainer();
  layerHost.style.backgroundColor = backgroundColor;

  for (const canvas of layerHost.querySelectorAll("canvas")) {
    canvas.style.backgroundColor = "transparent";
  }

  const webGLContexts = (
    renderer as unknown as {
      webGLContexts?: Record<string, WebGLRenderingContext>;
    }
  ).webGLContexts;

  if (!webGLContexts) {
    return;
  }

  webGLContexts.edges?.clearColor(1, 1, 1, 1);
  webGLContexts.nodes?.clearColor(1, 1, 1, 1);
  webGLContexts.hoverNodes?.clearColor(1, 1, 1, 1);
}
