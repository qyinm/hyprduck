use super::origin::*;
use super::replay::*;
use super::*;
use crate::graph_history::{graph_snapshot_source_ingest_id, latest_graph_materialized_event};

mod source_helpers;
mod snapshot_build;
mod memory_merge;
mod wiki_materialize;
mod read_model_files;
mod events;

pub(crate) use source_helpers::*;
pub(crate) use snapshot_build::*;
pub(crate) use memory_merge::*;
pub(crate) use wiki_materialize::*;
pub(crate) use read_model_files::*;
pub(crate) use events::*;

#[cfg(test)]
mod tests {
    use super::*;

    fn materialized_source_row() -> StoredSourceRow {
        StoredSourceRow {
            summary: hyprduck_engine_types::SourceSummary {
                workspace_id: "default".into(),
                source_id: "source-1".into(),
                original_path: "/tmp/source-1.pdf".into(),
                source_path: "/tmp/source-1.pdf".into(),
                markdown_path: "/tmp/source-1.md".into(),
                format: hyprduck_engine_types::DocumentFormat::Pdf,
                status: hyprduck_engine_types::IngestStatus::Ingested,
                page_count: 1,
                success_count: 1,
                failed_count: 0,
                citation_ready: true,
                graph_ready: true,
                description: String::new(),
                user_context: String::new(),
                ingest_instruction: String::new(),
                updated_at: 5,
            },
            project_id: "project-source-1".into(),
            manifest_path: "/tmp/source-1/manifest.json".into(),
        }
    }

    fn materialized_evidence() -> EvidenceRef {
        EvidenceRef {
            id: "ev-source-1-p1".into(),
            page_label: "Page 1".into(),
            page_index: Some(0),
            snippet: "Lifecycle metadata should stay stable.".into(),
            source_path: Some("/tmp/source-1.pdf".into()),
            source_id: Some("source-1".into()),
            markdown_path: Some("/tmp/source-1.md".into()),
            image_path: None,
            provenance: Some("test".into()),
        }
    }

    fn materialized_aggregate() -> KnowledgeProject {
        let evidence = materialized_evidence();
        let node = GraphNodeSummary {
            id: "concept-lifecycle".into(),
            label: "Lifecycle".into(),
            kind: GraphNodeKind::Concept,
            confidence: Some(0.9),
            related_count: 1,
            evidence_count: 1,
            position: GraphNodePosition { x: 0.0, y: 0.0 },
        };
        let edge = RelationEdgeSummary {
            id: "rel-lifecycle-source".into(),
            source_node_id: "concept-lifecycle".into(),
            target_node_id: "source:source-1".into(),
            kind: RelationKind::RelatedTo,
            label: "Related to".into(),
            confidence: Some(0.8),
            evidence_count: 1,
        };
        KnowledgeProject {
            summary: ProjectOverview {
                project_id: "workspace-default".into(),
                title: "Workspace knowledge".into(),
                status: ProjectStatus::Ready,
                stale: false,
                summary: String::new(),
                document_count: 1,
                node_count: 1,
                relationship_count: 1,
                evidence_count: 2,
                hidden_concept_count: 0,
                hidden_relation_count: 0,
            },
            nodes: vec![node.clone()],
            edges: vec![edge.clone()],
            details_by_node_id: BTreeMap::from([(
                node.id.clone(),
                GraphNodeDetail {
                    node,
                    canonical_name: "Lifecycle".into(),
                    aliases: vec!["Stable lifecycle".into()],
                    description: String::new(),
                    evidence: vec![evidence.clone()],
                    actions: Vec::new(),
                    source: None,
                },
            )]),
            edge_details_by_id: BTreeMap::from([(
                edge.id.clone(),
                RelationEdgeDetail {
                    edge,
                    explanation: "Lifecycle relation".into(),
                    evidence: vec![evidence],
                },
            )]),
            answer_by_node_id: BTreeMap::new(),
        }
    }

    #[test]
    fn materialization_sets_valid_from_on_new_graph_records() {
        let snapshot = build_brain_repo_snapshot(
            "default",
            &[(materialized_source_row(), None)],
            &materialized_aggregate(),
            &[],
            &[],
            &[],
            &[],
        );

        let node = snapshot
            .nodes
            .iter()
            .find(|node| node.node_id == "concept-lifecycle")
            .expect("materialized concept node");
        let relation = snapshot
            .relations
            .iter()
            .find(|relation| relation.relation_id == "rel-lifecycle-source")
            .expect("materialized relation");

        assert_eq!(node.valid_from, snapshot.generated_at);
        assert_eq!(node.valid_to, None);
        assert_eq!(node.superseded_by, None);
        assert_eq!(relation.valid_from, snapshot.generated_at);
        assert_eq!(relation.valid_to, None);
        assert_eq!(relation.superseded_by, None);
    }

    #[test]
    fn materialization_preserves_lifecycle_for_unchanged_records() {
        let existing_node = BrainNodeRecord {
            node_id: "concept-lifecycle".into(),
            kind: BrainNodeKind::Concept,
            label: "Lifecycle".into(),
            scope: BrainScope::Project,
            aliases: vec!["Stable lifecycle".into()],
            evidence_ids: vec!["ev-source-1-p1".into()],
            source_ids: vec!["source-1".into()],
            confidence: Some(0.9),
            updated_at: 11,
            valid_from: 7,
            valid_to: Some(13),
            superseded_by: Some("evt-existing".into()),
        };
        let existing_relation = BrainRelationRecord {
            relation_id: "rel-lifecycle-source".into(),
            kind: BrainRelationKind::RelatedTo,
            source_node_id: "concept-lifecycle".into(),
            target_node_id: "source:source-1".into(),
            label: "Related to".into(),
            evidence_ids: vec!["ev-source-1-p1".into()],
            confidence: Some(0.8),
            updated_at: 12,
            valid_from: 8,
            valid_to: Some(14),
            superseded_by: Some("evt-existing-relation".into()),
        };

        let snapshot = build_brain_repo_snapshot(
            "default",
            &[(materialized_source_row(), None)],
            &materialized_aggregate(),
            &[],
            &[],
            std::slice::from_ref(&existing_node),
            std::slice::from_ref(&existing_relation),
        );

        let node = snapshot
            .nodes
            .iter()
            .find(|node| node.node_id == existing_node.node_id)
            .expect("materialized concept node");
        let relation = snapshot
            .relations
            .iter()
            .find(|relation| relation.relation_id == existing_relation.relation_id)
            .expect("materialized relation");

        assert_eq!(node.updated_at, existing_node.updated_at);
        assert_eq!(node.valid_from, existing_node.valid_from);
        assert_eq!(node.valid_to, existing_node.valid_to);
        assert_eq!(node.superseded_by, existing_node.superseded_by);
        assert_eq!(relation.updated_at, existing_relation.updated_at);
        assert_eq!(relation.valid_from, existing_relation.valid_from);
        assert_eq!(relation.valid_to, existing_relation.valid_to);
        assert_eq!(relation.superseded_by, existing_relation.superseded_by);
    }
}
