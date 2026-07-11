import { Badge } from "@/components/ui/badge";
import { useI18n } from "@/i18n/I18nProvider";

import { EvidenceCard } from "./GraphEvidence";
import type { WorkspaceProject } from "./types";

interface GraphEdgeInspectorProps {
  selectedEdge: NonNullable<WorkspaceProject["edgeDetailsById"][string]>;
  nodeById: Record<string, WorkspaceProject["nodes"][number]>;
}

export function GraphEdgeInspector(props: GraphEdgeInspectorProps) {
  const { selectedEdge, nodeById } = props;
  const { t } = useI18n();

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-4 py-3">
      <section className="space-y-2 border-b border-border/70 pb-3">
        <div className="flex items-center gap-2">
          <Badge variant="outline">{t("workspace.inspector.connection")}</Badge>
          <Badge variant="secondary">
            {selectedEdge.edge.confidence === null
              ? t("workspace.inspector.evidenceBacked")
              : t("workspace.inspector.confidence", {
                  percent: Math.round(selectedEdge.edge.confidence * 100),
                })}
          </Badge>
        </div>
        <div>
          <h4 className="text-base font-semibold leading-6 tracking-tight">
            {t("workspace.inspector.connectsTo", {
              source:
                nodeById[selectedEdge.edge.sourceNodeId]?.label ??
                selectedEdge.edge.sourceNodeId,
              target:
                nodeById[selectedEdge.edge.targetNodeId]?.label ??
                selectedEdge.edge.targetNodeId,
            })}
          </h4>
          <p className="mt-1 line-clamp-2 text-sm leading-5 text-muted-foreground">
            {selectedEdge.explanation}
          </p>
        </div>
        <div className="flex flex-wrap gap-1.5">
          <span className="rounded-full bg-secondary px-2.5 py-1 text-xs text-secondary-foreground">
            {selectedEdge.edge.label}
          </span>
          <span className="rounded-full bg-secondary px-2.5 py-1 text-xs text-secondary-foreground">
            {selectedEdge.edge.evidenceCount} evidence
          </span>
        </div>
      </section>

      <section className="space-y-2">
        <div className="flex items-center justify-between">
          <h5 className="text-sm font-semibold">Why these are connected</h5>
          <span className="text-xs text-muted-foreground">
            {selectedEdge.evidence.length} evidence
          </span>
        </div>
        <div className="space-y-2">
          {selectedEdge.evidence.slice(0, 3).map((evidence) => (
            <EvidenceCard evidence={evidence} key={evidence.id} />
          ))}
        </div>
      </section>
    </div>
  );
}
