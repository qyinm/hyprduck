//! Internal helpers extracted from the engine facade module.

use std::path::Path;

use super::{
    read_latest_readable_graph_snapshot_marker, EngineConfig,
    ProviderGraphMaterializationInputFingerprint, SourceArtifactManifest,
    PROVIDER_GRAPH_PROMPT_VERSION, PROVIDER_SOURCE_GRAPH_SCHEMA_VERSION,
    PROVIDER_WORKSPACE_LINKING_SCHEMA_VERSION,
};

pub(super) fn provider_graph_input_fingerprint(
    workspace_root: &Path,
    workspace_id: &str,
    manifest: &SourceArtifactManifest,
    markdown: &str,
    config: &EngineConfig,
) -> ProviderGraphMaterializationInputFingerprint {
    let baseline_marker = read_latest_readable_graph_snapshot_marker(workspace_root)
        .ok()
        .flatten();
    ProviderGraphMaterializationInputFingerprint {
        workspace_id: workspace_id.into(),
        source_id: manifest.source_id.clone(),
        manifest_updated_at: manifest.updated_at,
        markdown_hash: stable_text_hash(markdown),
        provider: config.provider.id_slug().into(),
        model: config.model_id.clone(),
        source_graph_schema_version: PROVIDER_SOURCE_GRAPH_SCHEMA_VERSION,
        workspace_linking_schema_version: PROVIDER_WORKSPACE_LINKING_SCHEMA_VERSION,
        prompt_version: PROVIDER_GRAPH_PROMPT_VERSION,
        baseline_snapshot_id: baseline_marker
            .as_ref()
            .map(|marker| marker.snapshot_id.clone()),
        baseline_event_id: baseline_marker
            .as_ref()
            .map(|marker| marker.event_id.clone()),
        baseline_materialized_at: baseline_marker
            .as_ref()
            .map(|marker| marker.materialized_at),
    }
}

fn stable_text_hash(value: &str) -> String {
    format!("{:016x}", fnv1a_hash(value.as_bytes()))
}

fn fnv1a_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
