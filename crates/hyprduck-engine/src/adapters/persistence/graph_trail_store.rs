//! Graph trail loading and context-pack trail attachment.

use anyhow::{Context, Result};
use graphqlite::Graph;
use hyprduck_engine_types::{
    ContextPackEvidenceV1, ContextPackGraphFollowUpArgumentsV1, ContextPackGraphFollowUpToolV1,
    ContextPackGraphFollowUpV1, ContextPackGraphHandleTypeV1, ContextPackGraphReadNodeArgumentsV1,
    ContextPackGraphReadPageEvidenceArgumentsV1, ContextPackGraphReadSourceArgumentsV1,
    ContextPackGraphReadWikiPageArgumentsV1, ContextPackGraphRecordKindV1,
    ContextPackGraphRecordV1, ContextPackGraphTrailV1, ContextPackV1, ContextPackWarningSeverity,
    ContextPackWarningV0,
};
use std::collections::{BTreeMap, BTreeSet};

use super::context_pack_store::{
    load_context_pack_evidence_row, load_context_pack_source_row,
};
use crate::policy;

const GRAPH_TRAIL_DIRECT_LIMIT: usize = 8;
const GRAPH_TRAIL_ADJACENT_LIMIT: usize = 8;
const GRAPH_TRAIL_FOLLOW_UP_LIMIT: usize = 8;
const GRAPH_TRAIL_QUERY_OVERFETCH_MULTIPLIER: usize = 4;

#[derive(Debug, Clone)]
struct GraphTrailNode {
    node_id: String,
    kind: String,
    label: String,
    aliases: Vec<String>,
    evidence_ids: Vec<String>,
    source_ids: Vec<String>,
    status: String,
}

#[derive(Debug, Clone)]
struct GraphTrailRelation {
    relation_id: String,
    label: String,
    evidence_ids: Vec<String>,
    source_ids: Vec<String>,
}

#[derive(Debug, Clone)]
struct GraphTrailRelationLink {
    relation: GraphTrailRelation,
    source: GraphTrailNode,
    target: GraphTrailNode,
}

#[derive(Debug)]
struct GraphTrailIndex {
    nodes_by_evidence: BTreeMap<String, Vec<GraphTrailNode>>,
    relation_links_by_evidence: BTreeMap<String, Vec<GraphTrailRelationLink>>,
    relation_links_by_node: BTreeMap<String, Vec<GraphTrailRelationLink>>,
    eligible_evidence_ids: BTreeSet<String>,
    eligible_source_ids: BTreeSet<String>,
}

impl GraphTrailIndex {
    fn load(
        graph: &Graph,
        workspace_id: &str,
        selected_evidence_ids: &BTreeSet<String>,
        selected_source_ids: &BTreeSet<String>,
    ) -> Result<Self> {
        let (eligible_evidence_ids, eligible_source_ids) = load_graph_trail_eligible_refs(
            graph,
            workspace_id,
            selected_evidence_ids,
            selected_source_ids,
        )?;
        let mut index = Self {
            nodes_by_evidence: BTreeMap::new(),
            relation_links_by_evidence: BTreeMap::new(),
            relation_links_by_node: BTreeMap::new(),
            eligible_evidence_ids,
            eligible_source_ids,
        };

        for (evidence_id, nodes) in
            load_graph_trail_nodes_by_evidence(graph, workspace_id, &index.eligible_evidence_ids)?
        {
            for node in nodes {
                if index.node_is_eligible(&node) {
                    index
                        .nodes_by_evidence
                        .entry(evidence_id.clone())
                        .or_default()
                        .push(node);
                }
            }
        }
        for (evidence_id, links) in load_graph_trail_relation_links_by_evidence(
            graph,
            workspace_id,
            &index.eligible_evidence_ids,
        )? {
            for link in links {
                if index.relation_is_eligible(&link.relation) {
                    index
                        .relation_links_by_evidence
                        .entry(evidence_id.clone())
                        .or_default()
                        .push(link);
                }
            }
        }

        sort_and_limit_graph_trail_map(&mut index.nodes_by_evidence, |node| node.node_id.as_str());
        let direct_node_ids = index
            .nodes_by_evidence
            .values()
            .flat_map(|nodes| nodes.iter().map(|node| node.node_id.clone()))
            .collect::<BTreeSet<_>>();
        for node_id in direct_node_ids {
            for link in load_graph_trail_relation_links_for_node(graph, workspace_id, &node_id)? {
                if index.relation_is_eligible(&link.relation) {
                    index
                        .relation_links_by_node
                        .entry(node_id.clone())
                        .or_default()
                        .push(link);
                }
            }
        }

        sort_and_limit_graph_trail_map(&mut index.relation_links_by_evidence, |link| {
            link.relation.relation_id.as_str()
        });
        sort_and_limit_graph_trail_map(&mut index.relation_links_by_node, |link| {
            link.relation.relation_id.as_str()
        });

        Ok(index)
    }

    fn nodes_for_evidence(&self, evidence_id: &str) -> &[GraphTrailNode] {
        self.nodes_by_evidence
            .get(evidence_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn relation_links_for_evidence(&self, evidence_id: &str) -> &[GraphTrailRelationLink] {
        self.relation_links_by_evidence
            .get(evidence_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn relation_links_for_node(&self, node_id: &str) -> &[GraphTrailRelationLink] {
        self.relation_links_by_node
            .get(node_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn node_is_eligible(&self, node: &GraphTrailNode) -> bool {
        graph_trail_node_status_is_visible(&node.kind, &node.status)
            && policy::is_agent_text_safe(&node.node_id)
            && policy::is_agent_text_safe(&node.label)
            && self.refs_are_eligible(&node.evidence_ids, &node.source_ids)
    }

    fn relation_is_eligible(&self, relation: &GraphTrailRelation) -> bool {
        policy::is_agent_text_safe(&relation.relation_id)
            && policy::is_agent_text_safe(&relation.label)
            && self.refs_are_eligible(&relation.evidence_ids, &relation.source_ids)
    }

    fn refs_are_eligible(&self, evidence_ids: &[String], source_ids: &[String]) -> bool {
        if !evidence_ids.is_empty() {
            return evidence_ids
                .iter()
                .any(|evidence_id| self.eligible_evidence_ids.contains(evidence_id));
        }
        source_ids
            .iter()
            .any(|source_id| self.eligible_source_ids.contains(source_id))
    }
}

fn sort_and_limit_graph_trail_map<T, F>(records: &mut BTreeMap<String, Vec<T>>, key: F)
where
    F: Fn(&T) -> &str,
{
    for values in records.values_mut() {
        values.sort_by(|left, right| key(left).cmp(key(right)));
        values.dedup_by(|left, right| key(left) == key(right));
        values.truncate(GRAPH_TRAIL_DIRECT_LIMIT.max(GRAPH_TRAIL_ADJACENT_LIMIT));
    }
}

pub(super) fn attach_context_pack_graph_trails(
    graph: &Graph,
    workspace_id: &str,
    context_pack: &mut ContextPackV1,
) -> Result<()> {
    let selected_evidence_ids = context_pack
        .selected_evidence
        .iter()
        .map(|evidence| evidence.evidence_ref.clone())
        .collect::<BTreeSet<_>>();
    let selected_source_ids = context_pack
        .selected_evidence
        .iter()
        .map(|evidence| evidence.source_id.clone())
        .collect::<BTreeSet<_>>();
    let graph_trail_index = GraphTrailIndex::load(
        graph,
        workspace_id,
        &selected_evidence_ids,
        &selected_source_ids,
    )?;
    for evidence in &mut context_pack.selected_evidence {
        evidence.graph_trail = build_context_pack_graph_trail(&graph_trail_index, evidence)?;
    }
    Ok(())
}

fn build_context_pack_graph_trail(
    graph_trail_index: &GraphTrailIndex,
    evidence: &ContextPackEvidenceV1,
) -> Result<Option<ContextPackGraphTrailV1>> {
    let mut direct = Vec::new();
    let mut adjacent = Vec::new();
    let mut follow_up = Vec::new();
    let mut direct_keys = BTreeSet::new();
    let mut adjacent_keys = BTreeSet::new();
    let mut follow_up_keys = BTreeSet::new();

    let direct_nodes = graph_trail_index.nodes_for_evidence(&evidence.evidence_ref);
    for node in direct_nodes {
        push_graph_record(
            &mut direct,
            &mut direct_keys,
            GRAPH_TRAIL_DIRECT_LIMIT,
            graph_record_kind_for_node(&node.kind),
            node.node_id.clone(),
            format!(
                "Selected evidence directly supports graph node '{}'.",
                node.label
            ),
        );
    }

    let mut direct_relation_ids = BTreeSet::new();
    let direct_relation_links =
        graph_trail_index.relation_links_for_evidence(&evidence.evidence_ref);
    for link in direct_relation_links {
        let previous_direct_len = direct.len();
        push_graph_record(
            &mut direct,
            &mut direct_keys,
            GRAPH_TRAIL_DIRECT_LIMIT,
            ContextPackGraphRecordKindV1::Relation,
            link.relation.relation_id.clone(),
            format!(
                "Selected evidence directly supports relation '{}'.",
                link.relation.label
            ),
        );
        if direct.len() > previous_direct_len {
            direct_relation_ids.insert(link.relation.relation_id.clone());
        }
        push_adjacent_node_from_relation_endpoint(
            graph_trail_index,
            &link.source,
            &mut adjacent,
            &mut adjacent_keys,
        );
        push_adjacent_node_from_relation_endpoint(
            graph_trail_index,
            &link.target,
            &mut adjacent,
            &mut adjacent_keys,
        );
    }

    for node in direct_nodes {
        for link in graph_trail_index.relation_links_for_node(&node.node_id) {
            if direct_relation_ids.contains(&link.relation.relation_id) {
                continue;
            }
            push_graph_record(
                &mut adjacent,
                &mut adjacent_keys,
                GRAPH_TRAIL_ADJACENT_LIMIT,
                ContextPackGraphRecordKindV1::Relation,
                link.relation.relation_id.clone(),
                format!(
                    "Relation '{}' is adjacent to directly supported node '{}'.",
                    link.relation.label, node.label
                ),
            );
            let neighbor = if link.source.node_id == node.node_id {
                &link.target
            } else {
                &link.source
            };
            push_adjacent_node_from_relation_endpoint(
                graph_trail_index,
                neighbor,
                &mut adjacent,
                &mut adjacent_keys,
            );
        }
    }

    if direct.is_empty() && adjacent.is_empty() {
        return Ok(None);
    }

    push_context_pack_follow_up(
        &mut follow_up,
        &mut follow_up_keys,
        ContextPackGraphFollowUpV1 {
            tool: ContextPackGraphFollowUpToolV1::ReadSource,
            handle_type: ContextPackGraphHandleTypeV1::Source,
            arguments: ContextPackGraphFollowUpArgumentsV1::ReadSource(
                ContextPackGraphReadSourceArgumentsV1 {
                    source_id: evidence.source_id.clone(),
                },
            ),
            reason: "Read the source that contains this selected evidence.".into(),
        },
    );
    push_context_pack_follow_up(
        &mut follow_up,
        &mut follow_up_keys,
        ContextPackGraphFollowUpV1 {
            tool: ContextPackGraphFollowUpToolV1::ReadPageEvidence,
            handle_type: ContextPackGraphHandleTypeV1::PageEvidence,
            arguments: ContextPackGraphFollowUpArgumentsV1::ReadPageEvidence(
                ContextPackGraphReadPageEvidenceArgumentsV1 {
                    source_id: evidence.source_id.clone(),
                    page: evidence.page,
                },
            ),
            reason: "Read the page evidence behind this graph-linked selection.".into(),
        },
    );
    for node in direct_nodes.iter().chain(
        direct_relation_links
            .iter()
            .flat_map(|link| [&link.source, &link.target]),
    ) {
        if graph_trail_index.node_is_eligible(node) {
            push_follow_up_for_graph_node(node, &mut follow_up, &mut follow_up_keys);
        }
    }

    Ok(Some(ContextPackGraphTrailV1 {
        direct,
        adjacent,
        follow_up,
        unavailable_reason: None,
    }))
}

fn push_adjacent_node_from_relation_endpoint(
    graph_trail_index: &GraphTrailIndex,
    node: &GraphTrailNode,
    adjacent: &mut Vec<ContextPackGraphRecordV1>,
    adjacent_keys: &mut BTreeSet<String>,
) {
    if !graph_trail_index.node_is_eligible(node) {
        return;
    }
    push_graph_record(
        adjacent,
        adjacent_keys,
        GRAPH_TRAIL_ADJACENT_LIMIT,
        graph_record_kind_for_node(&node.kind),
        node.node_id.clone(),
        format!(
            "Graph node '{}' is adjacent to directly supported relation context.",
            node.label
        ),
    );
}

fn push_graph_record(
    records: &mut Vec<ContextPackGraphRecordV1>,
    seen: &mut BTreeSet<String>,
    limit: usize,
    record_type: ContextPackGraphRecordKindV1,
    id: String,
    reason: String,
) {
    if records.len() >= limit {
        return;
    }
    let key = format!("{record_type:?}:{id}");
    if !seen.insert(key) {
        return;
    }
    records.push(ContextPackGraphRecordV1 {
        record_type,
        id,
        reason,
    });
}

fn push_follow_up_for_graph_node(
    node: &GraphTrailNode,
    follow_up: &mut Vec<ContextPackGraphFollowUpV1>,
    seen: &mut BTreeSet<String>,
) {
    match graph_record_kind_for_node(&node.kind) {
        ContextPackGraphRecordKindV1::Node => push_context_pack_follow_up(
            follow_up,
            seen,
            ContextPackGraphFollowUpV1 {
                tool: ContextPackGraphFollowUpToolV1::ReadNode,
                handle_type: ContextPackGraphHandleTypeV1::Node,
                arguments: ContextPackGraphFollowUpArgumentsV1::ReadNode(
                    ContextPackGraphReadNodeArgumentsV1 {
                        node_id: node.node_id.clone(),
                    },
                ),
                reason: format!("Inspect graph node '{}'.", node.label),
            },
        ),
        ContextPackGraphRecordKindV1::Source => {
            let source_id = node
                .source_ids
                .first()
                .cloned()
                .or_else(|| node.node_id.strip_prefix("source:").map(ToOwned::to_owned));
            if let Some(source_id) = source_id {
                push_context_pack_follow_up(
                    follow_up,
                    seen,
                    ContextPackGraphFollowUpV1 {
                        tool: ContextPackGraphFollowUpToolV1::ReadSource,
                        handle_type: ContextPackGraphHandleTypeV1::Source,
                        arguments: ContextPackGraphFollowUpArgumentsV1::ReadSource(
                            ContextPackGraphReadSourceArgumentsV1 { source_id },
                        ),
                        reason: format!("Read source node '{}'.", node.label),
                    },
                );
            }
        }
        ContextPackGraphRecordKindV1::WikiPage => {
            if let Some(path) = node
                .aliases
                .iter()
                .find(|path| policy::is_safe_agent_wiki_path(path))
            {
                push_context_pack_follow_up(
                    follow_up,
                    seen,
                    ContextPackGraphFollowUpV1 {
                        tool: ContextPackGraphFollowUpToolV1::ReadWikiPage,
                        handle_type: ContextPackGraphHandleTypeV1::WikiPage,
                        arguments: ContextPackGraphFollowUpArgumentsV1::ReadWikiPage(
                            ContextPackGraphReadWikiPageArgumentsV1 { path: path.clone() },
                        ),
                        reason: format!("Read wiki page '{}'.", node.label),
                    },
                );
            }
        }
        ContextPackGraphRecordKindV1::Claim
        | ContextPackGraphRecordKindV1::Relation
        | ContextPackGraphRecordKindV1::Evidence => {}
    }
}

fn push_context_pack_follow_up(
    follow_up: &mut Vec<ContextPackGraphFollowUpV1>,
    seen: &mut BTreeSet<String>,
    item: ContextPackGraphFollowUpV1,
) {
    if follow_up.len() >= GRAPH_TRAIL_FOLLOW_UP_LIMIT {
        return;
    }
    let key = format!(
        "{:?}:{:?}:{:?}",
        item.tool, item.handle_type, item.arguments
    );
    if seen.insert(key) {
        follow_up.push(item);
    }
}

fn load_graph_trail_nodes_by_evidence(
    graph: &Graph,
    workspace_id: &str,
    evidence_ids: &BTreeSet<String>,
) -> Result<BTreeMap<String, Vec<GraphTrailNode>>> {
    let node_ids_by_evidence = graphqlite_ids_by_evidence_index(
        graph,
        workspace_id,
        "node",
        evidence_ids,
        graph_trail_query_limit(GRAPH_TRAIL_DIRECT_LIMIT),
    )?;
    let mut node_cache = BTreeMap::new();
    let mut nodes_by_evidence: BTreeMap<String, Vec<GraphTrailNode>> = BTreeMap::new();
    for (evidence_id, node_ids) in node_ids_by_evidence {
        let nodes = nodes_by_evidence.entry(evidence_id).or_default();
        for node_id in node_ids {
            if !node_cache.contains_key(&node_id) {
                node_cache.insert(
                    node_id,
                    load_graph_trail_node_by_internal_id(graph, workspace_id, node_id)?,
                );
            }
            if let Some(Some(node)) = node_cache.get(&node_id) {
                nodes.push(node.clone());
            }
        }
        nodes.sort_by(|left, right| left.node_id.cmp(&right.node_id));
    }
    Ok(nodes_by_evidence)
}

fn load_graph_trail_relation_links_by_evidence(
    graph: &Graph,
    workspace_id: &str,
    evidence_ids: &BTreeSet<String>,
) -> Result<BTreeMap<String, Vec<GraphTrailRelationLink>>> {
    let edge_ids_by_evidence = graphqlite_ids_by_evidence_index(
        graph,
        workspace_id,
        "edge",
        evidence_ids,
        graph_trail_query_limit(GRAPH_TRAIL_DIRECT_LIMIT),
    )?;
    let mut link_cache = BTreeMap::new();
    let mut links_by_evidence: BTreeMap<String, Vec<GraphTrailRelationLink>> = BTreeMap::new();
    for (evidence_id, edge_ids) in edge_ids_by_evidence {
        let links = links_by_evidence.entry(evidence_id).or_default();
        for edge_id in edge_ids {
            if !link_cache.contains_key(&edge_id) {
                link_cache.insert(
                    edge_id,
                    load_graph_trail_relation_link_by_edge_id(graph, workspace_id, edge_id)?,
                );
            }
            if let Some(Some(link)) = link_cache.get(&edge_id) {
                links.push(link.clone());
            }
        }
        links.sort_by(|left, right| left.relation.relation_id.cmp(&right.relation.relation_id));
    }
    Ok(links_by_evidence)
}

fn load_graph_trail_relation_links_for_node(
    graph: &Graph,
    workspace_id: &str,
    node_id: &str,
) -> Result<Vec<GraphTrailRelationLink>> {
    let Some(internal_node_id) = graphqlite_internal_node_id(graph, workspace_id, node_id)? else {
        return Ok(Vec::new());
    };
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare(
            "SELECT rowid
             FROM edges
             WHERE source_id = ?1 OR target_id = ?1
             ORDER BY rowid ASC
             LIMIT ?2",
        )
        .context("failed preparing GraphQLite node edge query")?;
    let mut rows = statement
        .query((
            internal_node_id,
            graph_trail_query_limit(GRAPH_TRAIL_ADJACENT_LIMIT) as i64,
        ))
        .context("failed querying GraphQLite node edges")?;
    let mut edge_ids = BTreeSet::new();
    while let Some(row) = rows
        .next()
        .context("failed reading GraphQLite node edge row")?
    {
        edge_ids.insert(row.get(0).context("read GraphQLite edge id")?);
    }
    let mut links = load_graph_trail_relation_links_for_edge_ids(graph, workspace_id, edge_ids)?;
    links.sort_by(|left, right| left.relation.relation_id.cmp(&right.relation.relation_id));
    links.dedup_by(|left, right| left.relation.relation_id == right.relation.relation_id);
    links.truncate(GRAPH_TRAIL_ADJACENT_LIMIT);
    Ok(links)
}

fn load_graph_trail_relation_links_for_edge_ids(
    graph: &Graph,
    workspace_id: &str,
    edge_ids: BTreeSet<i64>,
) -> Result<Vec<GraphTrailRelationLink>> {
    let mut links = Vec::new();
    for edge_id in edge_ids {
        if let Some(link) = load_graph_trail_relation_link_by_edge_id(graph, workspace_id, edge_id)?
        {
            links.push(link);
        }
    }
    Ok(links)
}

fn load_graph_trail_eligible_refs(
    graph: &Graph,
    workspace_id: &str,
    selected_evidence_ids: &BTreeSet<String>,
    selected_source_ids: &BTreeSet<String>,
) -> Result<(BTreeSet<String>, BTreeSet<String>)> {
    let mut eligible_evidence_ids = BTreeSet::new();
    for evidence_id in selected_evidence_ids {
        let Some(evidence_row) = load_context_pack_evidence_row(graph, workspace_id, evidence_id)?
        else {
            continue;
        };
        if load_context_pack_source_row(graph, workspace_id, &evidence_row.source_id)?.is_some() {
            eligible_evidence_ids.insert(evidence_id.clone());
        }
    }

    let mut eligible_source_ids = BTreeSet::new();
    for source_id in selected_source_ids {
        if load_context_pack_source_row(graph, workspace_id, source_id)?.is_some() {
            eligible_source_ids.insert(source_id.clone());
        }
    }

    Ok((eligible_evidence_ids, eligible_source_ids))
}

fn graph_trail_query_limit(limit: usize) -> usize {
    limit.saturating_mul(GRAPH_TRAIL_QUERY_OVERFETCH_MULTIPLIER)
}

fn graphqlite_ids_by_evidence_index(
    graph: &Graph,
    workspace_id: &str,
    record_kind: &str,
    ref_ids: &BTreeSet<String>,
    limit: usize,
) -> Result<BTreeMap<String, BTreeSet<i64>>> {
    if ref_ids.is_empty() || limit == 0 {
        return Ok(BTreeMap::new());
    }
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare(
            "SELECT record_internal_id
             FROM graph_evidence_record_index
             WHERE workspace_id = ?1
               AND evidence_id = ?2
               AND record_kind = ?3
             ORDER BY record_internal_id ASC
             LIMIT ?4",
        )
        .context("failed preparing graph evidence record index lookup")?;
    let mut ids_by_ref = ref_ids
        .iter()
        .map(|ref_id| (ref_id.clone(), BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    for ref_id in ref_ids {
        let mut rows = statement
            .query((workspace_id, ref_id.as_str(), record_kind, limit as i64))
            .with_context(|| format!("failed querying graph {record_kind} evidence index"))?;
        while let Some(row) = rows
            .next()
            .with_context(|| format!("failed reading graph {record_kind} evidence index row"))?
        {
            let record_internal_id = row
                .get(0)
                .context("read graph evidence record internal id")?;
            ids_by_ref
                .entry(ref_id.clone())
                .or_default()
                .insert(record_internal_id);
        }
    }
    ids_by_ref.retain(|_, ids| !ids.is_empty());
    Ok(ids_by_ref)
}

fn graphqlite_internal_node_id(
    graph: &Graph,
    workspace_id: &str,
    node_id: &str,
) -> Result<Option<i64>> {
    let Some(id_key_id) = graphqlite_text_property_key_id(graph, "id")? else {
        return Ok(None);
    };
    let Some(workspace_key_id) = graphqlite_text_property_key_id(graph, "workspace_id")? else {
        return Ok(None);
    };
    let logical_key_id = graphqlite_text_property_key_id(graph, "logical_id")?;
    let valid_to_key_id = graphqlite_text_property_key_id(graph, "valid_to")?;
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare(
            "SELECT id.node_id
             FROM node_props_text id
             JOIN node_props_text workspace
               ON workspace.node_id = id.node_id
              AND workspace.key_id = ?2
             LEFT JOIN node_props_text logical
               ON logical.node_id = id.node_id
              AND logical.key_id = ?3
             LEFT JOIN node_props_int valid_to
               ON valid_to.node_id = id.node_id
              AND valid_to.key_id = ?4
             WHERE id.key_id = ?1
               AND workspace.value = ?5
               AND COALESCE(NULLIF(logical.value, ''), id.value) = ?6
               AND COALESCE(valid_to.value, 0) <= 0
             ORDER BY CASE WHEN logical.value IS NULL OR logical.value = '' THEN 0 ELSE 1 END DESC,
                      id.node_id DESC
             LIMIT 1",
        )
        .context("failed preparing GraphQLite node id lookup")?;
    let mut rows = statement
        .query((
            id_key_id,
            workspace_key_id,
            logical_key_id.unwrap_or(0),
            valid_to_key_id.unwrap_or(0),
            workspace_id,
            node_id,
        ))
        .context("failed querying GraphQLite node id lookup")?;
    Ok(rows
        .next()
        .context("failed reading GraphQLite node id lookup row")?
        .map(|row| row.get(0).context("read GraphQLite internal node id"))
        .transpose()?)
}

fn graphqlite_text_property_key_id(graph: &Graph, key: &str) -> Result<Option<i64>> {
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare("SELECT id FROM property_keys WHERE key = ?1")
        .context("failed preparing GraphQLite property key lookup")?;
    let mut rows = statement
        .query([key])
        .context("failed querying GraphQLite property key lookup")?;
    Ok(rows
        .next()
        .context("failed reading GraphQLite property key lookup row")?
        .map(|row| row.get(0).context("read GraphQLite property key id"))
        .transpose()?)
}

fn load_graph_trail_relation_link_by_edge_id(
    graph: &Graph,
    workspace_id: &str,
    edge_id: i64,
) -> Result<Option<GraphTrailRelationLink>> {
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare("SELECT source_id, target_id FROM edges WHERE rowid = ?1")
        .context("failed preparing GraphQLite edge endpoint query")?;
    let mut rows = statement
        .query([edge_id])
        .context("failed querying GraphQLite edge endpoint")?;
    let Some(row) = rows
        .next()
        .context("failed reading GraphQLite edge endpoint row")?
    else {
        return Ok(None);
    };
    let source_id = row.get(0).context("read GraphQLite edge source id")?;
    let target_id = row.get(1).context("read GraphQLite edge target id")?;
    if !graphqlite_int_property_is_live(graph, "edge_props_int", "edge_id", edge_id)? {
        return Ok(None);
    }
    let props = graphqlite_edge_text_properties(graph, edge_id)?;
    if !graph_record_status_is_active(graphlite_prop(&props, "status")) {
        return Ok(None);
    }
    let Some(source) = load_graph_trail_node_by_internal_id(graph, workspace_id, source_id)? else {
        return Ok(None);
    };
    let Some(target) = load_graph_trail_node_by_internal_id(graph, workspace_id, target_id)? else {
        return Ok(None);
    };
    Ok(Some(GraphTrailRelationLink {
        relation: GraphTrailRelation {
            relation_id: graphlite_prop(&props, "relation_id").into(),
            label: graphlite_prop(&props, "label").into(),
            evidence_ids: graphlite_string_array_prop(&props, "evidence_ids_json")?,
            source_ids: graphlite_string_array_prop(&props, "source_ids_json")?,
        },
        source,
        target,
    }))
}

fn load_graph_trail_node_by_internal_id(
    graph: &Graph,
    workspace_id: &str,
    internal_node_id: i64,
) -> Result<Option<GraphTrailNode>> {
    let props = graphqlite_node_text_properties(graph, internal_node_id)?;
    if graphlite_prop(&props, "workspace_id") != workspace_id {
        return Ok(None);
    }
    if !graphqlite_int_property_is_live(graph, "node_props_int", "node_id", internal_node_id)? {
        return Ok(None);
    }
    let logical_id = graphlite_prop(&props, "logical_id");
    let physical_id = graphlite_prop(&props, "id");
    Ok(Some(GraphTrailNode {
        node_id: graph_trail_public_id(logical_id, physical_id).into(),
        kind: graphlite_prop(&props, "kind").into(),
        label: graphlite_prop(&props, "label").into(),
        aliases: graphlite_string_array_prop(&props, "aliases_json")?,
        evidence_ids: graphlite_string_array_prop(&props, "evidence_ids_json")?,
        source_ids: graphlite_string_array_prop(&props, "source_ids_json")?,
        status: graphlite_prop(&props, "status").into(),
    }))
}

fn graph_trail_public_id<'a>(logical_id: &'a str, physical_id: &'a str) -> &'a str {
    if logical_id.trim().is_empty() {
        physical_id
    } else {
        logical_id
    }
}

fn graphqlite_int_property_is_live(
    graph: &Graph,
    table: &str,
    id_column: &str,
    record_id: i64,
) -> Result<bool> {
    Ok(graphqlite_int_property(graph, table, id_column, record_id, "valid_to")? <= 0)
}

fn graphqlite_int_property(
    graph: &Graph,
    table: &str,
    id_column: &str,
    record_id: i64,
    key: &str,
) -> Result<i64> {
    let Some(key_id) = graphqlite_text_property_key_id(graph, key)? else {
        return Ok(0);
    };
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare(&format!(
            "SELECT value FROM {table} WHERE {id_column} = ?1 AND key_id = ?2"
        ))
        .with_context(|| format!("failed preparing GraphQLite {table} int property query"))?;
    let mut rows = statement
        .query((record_id, key_id))
        .with_context(|| format!("failed querying GraphQLite {table} int property"))?;
    Ok(rows
        .next()
        .with_context(|| format!("failed reading GraphQLite {table} int property"))?
        .map(|row| row.get(0).context("read GraphQLite int property"))
        .transpose()?
        .unwrap_or_default())
}

fn graphqlite_node_text_properties(
    graph: &Graph,
    node_id: i64,
) -> Result<BTreeMap<String, String>> {
    graphqlite_text_properties(graph, "node_props_text", "node_id", node_id)
}

fn graphqlite_edge_text_properties(
    graph: &Graph,
    edge_id: i64,
) -> Result<BTreeMap<String, String>> {
    graphqlite_text_properties(graph, "edge_props_text", "edge_id", edge_id)
}

fn graphqlite_text_properties(
    graph: &Graph,
    table: &str,
    id_column: &str,
    id: i64,
) -> Result<BTreeMap<String, String>> {
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare(&format!(
            "SELECT k.key, p.value
             FROM {table} p
             JOIN property_keys k ON k.id = p.key_id
             WHERE p.{id_column} = ?1"
        ))
        .with_context(|| format!("failed preparing GraphQLite {table} properties query"))?;
    let mut rows = statement
        .query([id])
        .with_context(|| format!("failed querying GraphQLite {table} properties"))?;
    let mut props = BTreeMap::new();
    while let Some(row) = rows
        .next()
        .with_context(|| format!("failed reading GraphQLite {table} property row"))?
    {
        props.insert(
            row.get(0).context("read GraphQLite property key")?,
            row.get(1).context("read GraphQLite property value")?,
        );
    }
    Ok(props)
}

fn graphlite_prop<'a>(props: &'a BTreeMap<String, String>, key: &str) -> &'a str {
    props.get(key).map(String::as_str).unwrap_or_default()
}

fn graphlite_string_array_prop(props: &BTreeMap<String, String>, key: &str) -> Result<Vec<String>> {
    let value = graphlite_prop(props, key);
    if value.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(value).with_context(|| format!("failed decoding GraphQLite {key}"))
}

pub(super) fn graph_trail_unavailable_warning(message: &str) -> ContextPackWarningV0 {
    ContextPackWarningV0 {
        warning_type: "graph_trail_unavailable".into(),
        severity: ContextPackWarningSeverity::Low,
        message: message.into(),
        page_refs: Vec::new(),
    }
}

fn graph_trail_node_status_is_visible(kind: &str, status: &str) -> bool {
    graph_record_status_is_active(status) || (kind == "claim" && status == "supported")
}

fn graph_record_kind_for_node(kind: &str) -> ContextPackGraphRecordKindV1 {
    match kind {
        "claim" => ContextPackGraphRecordKindV1::Claim,
        "wiki_page" => ContextPackGraphRecordKindV1::WikiPage,
        "source" => ContextPackGraphRecordKindV1::Source,
        _ => ContextPackGraphRecordKindV1::Node,
    }
}

fn graph_record_status_is_active(status: &str) -> bool {
    status.is_empty() || status == "active"
}
