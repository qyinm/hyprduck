use anyhow::{Context, Result};
use graphqlite::Graph;
use hyprduck_engine_types::{BrainNodeKind, BrainRelationKind, BrainScope};

use super::GraphRecordVersionIdentity;

pub(super) fn ensure_graphlite_property_key_id(graph: &Graph, key: &str) -> Result<i64> {
    let sqlite = graph.connection().sqlite_connection();
    sqlite
        .execute(
            "INSERT OR IGNORE INTO property_keys (key) VALUES (?1)",
            [key],
        )
        .with_context(|| format!("failed ensuring GraphQLite property key {key}"))?;
    sqlite
        .query_row(
            "SELECT id FROM property_keys WHERE key = ?1",
            [key],
            |row| row.get(0),
        )
        .with_context(|| format!("failed reading GraphQLite property key {key}"))
}

pub(super) fn set_graphlite_int_property(
    graph: &Graph,
    table: &str,
    id_column: &str,
    record_id: i64,
    key: &str,
    value: i64,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    let key_id = ensure_graphlite_property_key_id(graph, key)?;
    sqlite
        .execute(
            &format!(
                "INSERT OR REPLACE INTO {table} ({id_column}, key_id, value) VALUES (?1, ?2, ?3)"
            ),
            (record_id, key_id, value),
        )
        .with_context(|| format!("failed setting GraphQLite int property {key}"))
        .map(|_| ())
}

pub(super) fn set_graphlite_text_property(
    graph: &Graph,
    table: &str,
    id_column: &str,
    record_id: i64,
    key: &str,
    value: &str,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    let key_id = ensure_graphlite_property_key_id(graph, key)?;
    sqlite
        .execute(
            &format!(
                "INSERT OR REPLACE INTO {table} ({id_column}, key_id, value) VALUES (?1, ?2, ?3)"
            ),
            (record_id, key_id, value),
        )
        .with_context(|| format!("failed setting GraphQLite text property {key}"))
        .map(|_| ())
}

pub(super) fn brain_node_label(kind: BrainNodeKind) -> &'static str {
    match kind {
        BrainNodeKind::Source => "Source",
        BrainNodeKind::Memory => "Memory",
        BrainNodeKind::WikiPage => "WikiPage",
        BrainNodeKind::Person => "Person",
        BrainNodeKind::Company => "Company",
        BrainNodeKind::Project => "Project",
        BrainNodeKind::Product => "Product",
        BrainNodeKind::Team => "Team",
        BrainNodeKind::Event => "Event",
        BrainNodeKind::Decision => "Decision",
        BrainNodeKind::Task => "Task",
        BrainNodeKind::Claim => "Claim",
        BrainNodeKind::Topic => "Topic",
        BrainNodeKind::Concept => "Concept",
    }
}

pub(super) fn brain_node_kind_slug(kind: BrainNodeKind) -> &'static str {
    match kind {
        BrainNodeKind::Source => "source",
        BrainNodeKind::Memory => "memory",
        BrainNodeKind::WikiPage => "wiki_page",
        BrainNodeKind::Person => "person",
        BrainNodeKind::Company => "company",
        BrainNodeKind::Project => "project",
        BrainNodeKind::Product => "product",
        BrainNodeKind::Team => "team",
        BrainNodeKind::Event => "event",
        BrainNodeKind::Decision => "decision",
        BrainNodeKind::Task => "task",
        BrainNodeKind::Claim => "claim",
        BrainNodeKind::Topic => "topic",
        BrainNodeKind::Concept => "concept",
    }
}

pub(super) fn brain_scope_slug(scope: BrainScope) -> &'static str {
    match scope {
        BrainScope::Personal => "personal",
        BrainScope::Project => "project",
        BrainScope::Team => "team",
        BrainScope::Company => "company",
    }
}

pub(super) fn brain_relation_type(kind: BrainRelationKind) -> &'static str {
    match kind {
        BrainRelationKind::Mentions => "MENTIONS",
        BrainRelationKind::Supports => "SUPPORTS",
        BrainRelationKind::Contradicts => "CONTRADICTS",
        BrainRelationKind::Supersedes => "SUPERSEDES",
        BrainRelationKind::SameAs => "SAME_AS",
        BrainRelationKind::WorksAt => "WORKS_AT",
        BrainRelationKind::Founded => "FOUNDED",
        BrainRelationKind::InvestedIn => "INVESTED_IN",
        BrainRelationKind::Advises => "ADVISES",
        BrainRelationKind::Attended => "ATTENDED",
        BrainRelationKind::Owns => "OWNS",
        BrainRelationKind::ResponsibleFor => "RESPONSIBLE_FOR",
        BrainRelationKind::Decided => "DECIDED",
        BrainRelationKind::Blocks => "BLOCKS",
        BrainRelationKind::DependsOn => "DEPENDS_ON",
        BrainRelationKind::SourceOf => "SOURCE_OF",
        BrainRelationKind::DerivedFrom => "DERIVED_FROM",
        BrainRelationKind::Cites => "CITES",
        BrainRelationKind::LinksTo => "LINKS_TO",
        BrainRelationKind::RelatedTo => "RELATED_TO",
    }
}

pub(super) fn graph_relation_version_type(
    kind: BrainRelationKind,
    identity: &GraphRecordVersionIdentity,
) -> String {
    let suffix = identity
        .version_id
        .rsplit('-')
        .next()
        .unwrap_or(identity.version_id.as_str());
    let suffix = suffix.get(..16).unwrap_or(suffix);
    format!("{}_V_{}", brain_relation_type(kind), suffix)
}

pub(super) fn brain_relation_kind_slug(kind: BrainRelationKind) -> &'static str {
    match kind {
        BrainRelationKind::Mentions => "mentions",
        BrainRelationKind::Supports => "supports",
        BrainRelationKind::Contradicts => "contradicts",
        BrainRelationKind::Supersedes => "supersedes",
        BrainRelationKind::SameAs => "same_as",
        BrainRelationKind::WorksAt => "works_at",
        BrainRelationKind::Founded => "founded",
        BrainRelationKind::InvestedIn => "invested_in",
        BrainRelationKind::Advises => "advises",
        BrainRelationKind::Attended => "attended",
        BrainRelationKind::Owns => "owns",
        BrainRelationKind::ResponsibleFor => "responsible_for",
        BrainRelationKind::Decided => "decided",
        BrainRelationKind::Blocks => "blocks",
        BrainRelationKind::DependsOn => "depends_on",
        BrainRelationKind::SourceOf => "source_of",
        BrainRelationKind::DerivedFrom => "derived_from",
        BrainRelationKind::Cites => "cites",
        BrainRelationKind::LinksTo => "links_to",
        BrainRelationKind::RelatedTo => "related_to",
    }
}
