//! Cloud graph plane store (S-PG4).
//!
//! Relational projection of nodes / relations / claims in Postgres.
//! GraphQLite remains local/engine-only and is never opened by etyma-server.
//! Full engine materialize wiring is S7 (PON-11); this module is the durable
//! cloud write/read boundary materialize will target.
//!
//! ## Live projection protocol
//!
//! Each logical id has at most one live row (`valid_to IS NULL`), enforced by a
//! partial unique index. Writes lock that live row (`FOR UPDATE`), reject
//! non-monotonic `valid_from`, close the prior version, then insert the new
//! live version — all in one transaction.

use serde::Serialize;
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::HashMap;
use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeRow {
    pub version_id: String,
    pub workspace_id: String,
    pub logical_id: String,
    pub kind: String,
    pub label: String,
    pub scope: String,
    pub aliases: Vec<String>,
    pub evidence_ids: Vec<String>,
    pub source_ids: Vec<String>,
    pub confidence: Option<f64>,
    pub created_by_event_id: Option<String>,
    pub valid_from: i64,
    pub valid_to: Option<i64>,
    pub superseded_by: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationRow {
    pub version_id: String,
    pub workspace_id: String,
    pub logical_id: String,
    pub kind: String,
    pub source_logical_id: String,
    pub target_logical_id: String,
    pub label: String,
    pub evidence_ids: Vec<String>,
    pub confidence: Option<f64>,
    pub created_by_event_id: Option<String>,
    pub valid_from: i64,
    pub valid_to: Option<i64>,
    pub superseded_by: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimRow {
    pub version_id: String,
    pub workspace_id: String,
    pub logical_id: String,
    pub statement: String,
    pub status: String,
    pub topic_refs: Vec<String>,
    pub source_refs: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub created_by_event_id: Option<String>,
    pub valid_from: i64,
    pub valid_to: Option<i64>,
    pub superseded_by: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct GraphSnapshot {
    pub workspace_id: String,
    pub nodes: Vec<NodeRow>,
    pub relations: Vec<RelationRow>,
    pub claims: Vec<ClaimRow>,
}

/// Public live-snapshot wire shape (no historical columns).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphSnapshotResponse {
    pub workspace_id: String,
    pub projection: &'static str,
    pub store: &'static str,
    pub nodes: Vec<LiveNodeView>,
    pub relations: Vec<LiveRelationView>,
    pub claims: Vec<LiveClaimView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveNodeView {
    pub version_id: String,
    pub id: String,
    pub kind: String,
    pub label: String,
    pub scope: String,
    pub aliases: Vec<String>,
    pub evidence_ids: Vec<String>,
    pub source_ids: Vec<String>,
    pub confidence: Option<f64>,
    pub created_by_event_id: Option<String>,
    pub valid_from: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveRelationView {
    pub version_id: String,
    pub id: String,
    pub kind: String,
    pub source_node_id: String,
    pub target_node_id: String,
    pub label: String,
    pub evidence_ids: Vec<String>,
    pub confidence: Option<f64>,
    pub created_by_event_id: Option<String>,
    pub valid_from: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveClaimView {
    pub version_id: String,
    pub id: String,
    pub statement: String,
    pub status: String,
    pub topic_refs: Vec<String>,
    pub source_refs: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub created_by_event_id: Option<String>,
    pub valid_from: i64,
}

impl From<GraphSnapshot> for GraphSnapshotResponse {
    fn from(snap: GraphSnapshot) -> Self {
        Self {
            workspace_id: snap.workspace_id,
            projection: "live",
            store: "postgres.graph",
            nodes: snap
                .nodes
                .into_iter()
                .map(|n| LiveNodeView {
                    version_id: n.version_id,
                    id: n.logical_id,
                    kind: n.kind,
                    label: n.label,
                    scope: n.scope,
                    aliases: n.aliases,
                    evidence_ids: n.evidence_ids,
                    source_ids: n.source_ids,
                    confidence: n.confidence,
                    created_by_event_id: n.created_by_event_id,
                    valid_from: n.valid_from,
                })
                .collect(),
            relations: snap
                .relations
                .into_iter()
                .map(|r| LiveRelationView {
                    version_id: r.version_id,
                    id: r.logical_id,
                    kind: r.kind,
                    source_node_id: r.source_logical_id,
                    target_node_id: r.target_logical_id,
                    label: r.label,
                    evidence_ids: r.evidence_ids,
                    confidence: r.confidence,
                    created_by_event_id: r.created_by_event_id,
                    valid_from: r.valid_from,
                })
                .collect(),
            claims: snap
                .claims
                .into_iter()
                .map(|c| LiveClaimView {
                    version_id: c.version_id,
                    id: c.logical_id,
                    statement: c.statement,
                    status: c.status,
                    topic_refs: c.topic_refs,
                    source_refs: c.source_refs,
                    evidence_refs: c.evidence_refs,
                    created_by_event_id: c.created_by_event_id,
                    valid_from: c.valid_from,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpsertNode {
    pub logical_id: String,
    pub kind: String,
    pub label: String,
    pub scope: String,
    pub aliases: Vec<String>,
    pub evidence_ids: Vec<String>,
    pub source_ids: Vec<String>,
    pub confidence: Option<f64>,
    pub created_by_event_id: Option<String>,
    /// Materialized version counter / event ordinal for `valid_from`.
    pub valid_from: i64,
}

#[derive(Debug, Clone)]
pub struct UpsertRelation {
    pub logical_id: String,
    pub kind: String,
    pub source_logical_id: String,
    pub target_logical_id: String,
    pub label: String,
    pub evidence_ids: Vec<String>,
    pub confidence: Option<f64>,
    pub created_by_event_id: Option<String>,
    pub valid_from: i64,
}

#[derive(Debug, Clone)]
pub struct UpsertClaim {
    pub logical_id: String,
    pub statement: String,
    pub status: String,
    pub topic_refs: Vec<String>,
    pub source_refs: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub created_by_event_id: Option<String>,
    pub valid_from: i64,
}

#[derive(Debug)]
pub enum GraphError {
    Conflict(String),
    Internal(String),
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict(message) | Self::Internal(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for GraphError {}

impl From<GraphError> for crate::store::StoreError {
    fn from(error: GraphError) -> Self {
        match error {
            GraphError::Conflict(message) => Self::Conflict(message),
            GraphError::Internal(message) => Self::Internal(message),
        }
    }
}

pub type GraphResult<T> = Result<T, GraphError>;

#[derive(Clone)]
pub struct GraphStore {
    pool: PgPool,
}

impl GraphStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Insert a new live node version, superseding any previous live version.
    pub async fn upsert_live_node(
        &self,
        workspace_id: &str,
        input: &UpsertNode,
    ) -> GraphResult<NodeRow> {
        let mut tx = self.pool.begin().await.map_err(map_tx)?;
        let row = upsert_live_node_tx(&mut tx, workspace_id, input).await?;
        tx.commit().await.map_err(map_tx)?;
        Ok(row)
    }

    pub async fn upsert_live_relation(
        &self,
        workspace_id: &str,
        input: &UpsertRelation,
    ) -> GraphResult<RelationRow> {
        let mut tx = self.pool.begin().await.map_err(map_tx)?;
        let row = upsert_live_relation_tx(&mut tx, workspace_id, input).await?;
        tx.commit().await.map_err(map_tx)?;
        Ok(row)
    }

    pub async fn upsert_live_claim(
        &self,
        workspace_id: &str,
        input: &UpsertClaim,
    ) -> GraphResult<ClaimRow> {
        let mut tx = self.pool.begin().await.map_err(map_tx)?;
        let row = upsert_live_claim_tx(&mut tx, workspace_id, input).await?;
        tx.commit().await.map_err(map_tx)?;
        Ok(row)
    }

    /// Atomically replace the entire live projection for a workspace.
    ///
    /// Closes every live row, then inserts the provided set. Endpoint existence
    /// for relations is **not** enforced — the writer (materialize) owns graph
    /// consistency. Callers must pass a single monotonic `materialized_version`
    /// greater than any existing live `valid_from` in the workspace.
    pub async fn replace_live_projection(
        &self,
        workspace_id: &str,
        materialized_version: i64,
        created_by_event_id: &str,
        nodes: &[UpsertNode],
        relations: &[UpsertRelation],
        claims: &[UpsertClaim],
    ) -> GraphResult<GraphSnapshot> {
        let mut tx = self.pool.begin().await.map_err(map_tx)?;
        let now = unix_now();

        close_all_live(
            &mut tx,
            workspace_id,
            materialized_version,
            created_by_event_id,
            now,
        )
        .await?;

        for node in nodes {
            let mut input = node.clone();
            input.valid_from = materialized_version;
            input.created_by_event_id = Some(created_by_event_id.to_string());
            insert_node_live(&mut tx, workspace_id, &input, new_version_id("nv"), now).await?;
        }
        for rel in relations {
            let mut input = rel.clone();
            input.valid_from = materialized_version;
            input.created_by_event_id = Some(created_by_event_id.to_string());
            insert_relation_live(&mut tx, workspace_id, &input, new_version_id("rv"), now)
                .await?;
        }
        for claim in claims {
            let mut input = claim.clone();
            input.valid_from = materialized_version;
            input.created_by_event_id = Some(created_by_event_id.to_string());
            insert_claim_live(&mut tx, workspace_id, &input, new_version_id("cv"), now).await?;
        }

        tx.commit().await.map_err(map_tx)?;
        self.live_snapshot(workspace_id).await
    }

    /// Live projection only (`valid_to IS NULL`). No GraphQLite open.
    pub async fn live_snapshot(&self, workspace_id: &str) -> GraphResult<GraphSnapshot> {
        let nodes_f = sqlx::query_as::<_, NodeRow>(
            r#"
            SELECT version_id, workspace_id, logical_id, kind, label, scope,
                   aliases, evidence_ids, source_ids, confidence,
                   created_by_event_id, valid_from, valid_to, superseded_by, updated_at
            FROM graph.nodes
            WHERE workspace_id = $1 AND valid_to IS NULL
            ORDER BY logical_id
            "#,
        )
        .bind(workspace_id)
        .fetch_all(&self.pool);

        let relations_f = sqlx::query_as::<_, RelationRow>(
            r#"
            SELECT version_id, workspace_id, logical_id, kind,
                   source_logical_id, target_logical_id, label, evidence_ids, confidence,
                   created_by_event_id, valid_from, valid_to, superseded_by, updated_at
            FROM graph.relations
            WHERE workspace_id = $1 AND valid_to IS NULL
            ORDER BY logical_id
            "#,
        )
        .bind(workspace_id)
        .fetch_all(&self.pool);

        let claims_f = sqlx::query_as::<_, ClaimRow>(
            r#"
            SELECT version_id, workspace_id, logical_id, statement, status,
                   topic_refs, source_refs, evidence_refs,
                   created_by_event_id, valid_from, valid_to, superseded_by, updated_at
            FROM graph.claims
            WHERE workspace_id = $1 AND valid_to IS NULL
            ORDER BY logical_id
            "#,
        )
        .bind(workspace_id)
        .fetch_all(&self.pool);

        let (nodes, relations, claims) = tokio::try_join!(
            async { nodes_f.await.map_err(map_read) },
            async { relations_f.await.map_err(map_read) },
            async { claims_f.await.map_err(map_read) },
        )?;

        Ok(GraphSnapshot {
            workspace_id: workspace_id.to_string(),
            nodes,
            relations,
            claims,
        })
    }

    /// Live nodes that cite any of the given evidence ids (batch; for pack trails).
    pub async fn live_nodes_for_evidence_ids(
        &self,
        workspace_id: &str,
        evidence_ids: &[String],
    ) -> GraphResult<Vec<NodeRow>> {
        if evidence_ids.is_empty() {
            return Ok(vec![]);
        }
        sqlx::query_as::<_, NodeRow>(
            r#"
            SELECT version_id, workspace_id, logical_id, kind, label, scope,
                   aliases, evidence_ids, source_ids, confidence,
                   created_by_event_id, valid_from, valid_to, superseded_by, updated_at
            FROM graph.nodes
            WHERE workspace_id = $1
              AND valid_to IS NULL
              AND evidence_ids && $2::text[]
            ORDER BY logical_id
            "#,
        )
        .bind(workspace_id)
        .bind(evidence_ids)
        .fetch_all(&self.pool)
        .await
        .map_err(map_read)
    }

    /// Live nodes that cite the given evidence id.
    pub async fn live_nodes_for_evidence(
        &self,
        workspace_id: &str,
        evidence_id: &str,
    ) -> GraphResult<Vec<NodeRow>> {
        self.live_nodes_for_evidence_ids(workspace_id, &[evidence_id.to_string()])
            .await
    }

    /// Adjacent live relations for a set of node logical ids.
    pub async fn live_relations_touching(
        &self,
        workspace_id: &str,
        node_ids: &[String],
    ) -> GraphResult<Vec<RelationRow>> {
        if node_ids.is_empty() {
            return Ok(vec![]);
        }
        sqlx::query_as::<_, RelationRow>(
            r#"
            SELECT version_id, workspace_id, logical_id, kind,
                   source_logical_id, target_logical_id, label, evidence_ids, confidence,
                   created_by_event_id, valid_from, valid_to, superseded_by, updated_at
            FROM graph.relations
            WHERE workspace_id = $1
              AND valid_to IS NULL
              AND (source_logical_id = ANY($2) OR target_logical_id = ANY($2))
            ORDER BY logical_id
            "#,
        )
        .bind(workspace_id)
        .bind(node_ids)
        .fetch_all(&self.pool)
        .await
        .map_err(map_read)
    }

    /// Map evidence id → live nodes that cite it (from a prefetched node list).
    pub fn index_nodes_by_evidence(nodes: &[NodeRow]) -> HashMap<String, Vec<&NodeRow>> {
        let mut map: HashMap<String, Vec<&NodeRow>> = HashMap::new();
        for node in nodes {
            for ev in &node.evidence_ids {
                map.entry(ev.clone()).or_default().push(node);
            }
        }
        map
    }
}

// --- versioned live write protocol (shared) ---------------------------------

async fn upsert_live_node_tx(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: &str,
    input: &UpsertNode,
) -> GraphResult<NodeRow> {
    let now = unix_now();
    let version_id = new_version_id("nv");
    let supersede_by = supersede_marker(input.created_by_event_id.as_deref(), &version_id);
    close_live_row(
        tx,
        LiveTable::Nodes,
        workspace_id,
        &input.logical_id,
        input.valid_from,
        &supersede_by,
        now,
    )
    .await?;
    insert_node_live(tx, workspace_id, input, version_id, now).await
}

async fn upsert_live_relation_tx(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: &str,
    input: &UpsertRelation,
) -> GraphResult<RelationRow> {
    let now = unix_now();
    let version_id = new_version_id("rv");
    let supersede_by = supersede_marker(input.created_by_event_id.as_deref(), &version_id);
    close_live_row(
        tx,
        LiveTable::Relations,
        workspace_id,
        &input.logical_id,
        input.valid_from,
        &supersede_by,
        now,
    )
    .await?;
    insert_relation_live(tx, workspace_id, input, version_id, now).await
}

async fn upsert_live_claim_tx(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: &str,
    input: &UpsertClaim,
) -> GraphResult<ClaimRow> {
    let now = unix_now();
    let version_id = new_version_id("cv");
    let supersede_by = supersede_marker(input.created_by_event_id.as_deref(), &version_id);
    close_live_row(
        tx,
        LiveTable::Claims,
        workspace_id,
        &input.logical_id,
        input.valid_from,
        &supersede_by,
        now,
    )
    .await?;
    insert_claim_live(tx, workspace_id, input, version_id, now).await
}

#[derive(Clone, Copy)]
enum LiveTable {
    Nodes,
    Relations,
    Claims,
}

impl LiveTable {
    fn name(self) -> &'static str {
        match self {
            Self::Nodes => "nodes",
            Self::Relations => "relations",
            Self::Claims => "claims",
        }
    }
}

/// Lock live row (if any), enforce monotonic `valid_from`, close it.
async fn close_live_row(
    tx: &mut Transaction<'_, Postgres>,
    table: LiveTable,
    workspace_id: &str,
    logical_id: &str,
    valid_from: i64,
    supersede_by: &str,
    now: i64,
) -> GraphResult<()> {
    let prior = lock_live_valid_from(tx, table, workspace_id, logical_id).await?;
    if let Some(prior_from) = prior {
        if valid_from <= prior_from {
            return Err(GraphError::Conflict(format!(
                "non-monotonic graph {} version for {logical_id}: valid_from {valid_from} <= live {prior_from}",
                table.name()
            )));
        }
        close_one_live(
            tx,
            table,
            workspace_id,
            logical_id,
            valid_from,
            supersede_by,
            now,
        )
        .await?;
    }
    Ok(())
}

async fn lock_live_valid_from(
    tx: &mut Transaction<'_, Postgres>,
    table: LiveTable,
    workspace_id: &str,
    logical_id: &str,
) -> GraphResult<Option<i64>> {
    let result = match table {
        LiveTable::Nodes => {
            sqlx::query_scalar::<_, i64>(
                r#"
                SELECT valid_from FROM graph.nodes
                WHERE workspace_id = $1 AND logical_id = $2 AND valid_to IS NULL
                FOR UPDATE
                "#,
            )
            .bind(workspace_id)
            .bind(logical_id)
            .fetch_optional(&mut **tx)
            .await
        }
        LiveTable::Relations => {
            sqlx::query_scalar::<_, i64>(
                r#"
                SELECT valid_from FROM graph.relations
                WHERE workspace_id = $1 AND logical_id = $2 AND valid_to IS NULL
                FOR UPDATE
                "#,
            )
            .bind(workspace_id)
            .bind(logical_id)
            .fetch_optional(&mut **tx)
            .await
        }
        LiveTable::Claims => {
            sqlx::query_scalar::<_, i64>(
                r#"
                SELECT valid_from FROM graph.claims
                WHERE workspace_id = $1 AND logical_id = $2 AND valid_to IS NULL
                FOR UPDATE
                "#,
            )
            .bind(workspace_id)
            .bind(logical_id)
            .fetch_optional(&mut **tx)
            .await
        }
    };
    result.map_err(|e| map_write(e, table.name()))
}

async fn close_one_live(
    tx: &mut Transaction<'_, Postgres>,
    table: LiveTable,
    workspace_id: &str,
    logical_id: &str,
    valid_to: i64,
    supersede_by: &str,
    now: i64,
) -> GraphResult<()> {
    let result = match table {
        LiveTable::Nodes => {
            sqlx::query(
                r#"
                UPDATE graph.nodes
                SET valid_to = $3, superseded_by = $4, updated_at = $5
                WHERE workspace_id = $1 AND logical_id = $2 AND valid_to IS NULL
                "#,
            )
            .bind(workspace_id)
            .bind(logical_id)
            .bind(valid_to)
            .bind(supersede_by)
            .bind(now)
            .execute(&mut **tx)
            .await
        }
        LiveTable::Relations => {
            sqlx::query(
                r#"
                UPDATE graph.relations
                SET valid_to = $3, superseded_by = $4, updated_at = $5
                WHERE workspace_id = $1 AND logical_id = $2 AND valid_to IS NULL
                "#,
            )
            .bind(workspace_id)
            .bind(logical_id)
            .bind(valid_to)
            .bind(supersede_by)
            .bind(now)
            .execute(&mut **tx)
            .await
        }
        LiveTable::Claims => {
            sqlx::query(
                r#"
                UPDATE graph.claims
                SET valid_to = $3, superseded_by = $4, updated_at = $5
                WHERE workspace_id = $1 AND logical_id = $2 AND valid_to IS NULL
                "#,
            )
            .bind(workspace_id)
            .bind(logical_id)
            .bind(valid_to)
            .bind(supersede_by)
            .bind(now)
            .execute(&mut **tx)
            .await
        }
    };
    result.map(|_| ()).map_err(|e| map_write(e, table.name()))
}

/// Close every live row in the workspace when `materialized_version` is strictly greater.
async fn close_all_live(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: &str,
    materialized_version: i64,
    superseded_by: &str,
    now: i64,
) -> GraphResult<()> {
    for table in [LiveTable::Nodes, LiveTable::Relations, LiveTable::Claims] {
        let live_from = lock_all_live_valid_from(tx, table, workspace_id).await?;
        if let Some(max_live) = live_from.into_iter().max() {
            if materialized_version <= max_live {
                return Err(GraphError::Conflict(format!(
                    "non-monotonic replace for graph.{}: version {materialized_version} <= live max {max_live}",
                    table.name()
                )));
            }
        }
        close_all_live_rows(tx, table, workspace_id, materialized_version, superseded_by, now)
            .await?;
    }
    Ok(())
}

async fn lock_all_live_valid_from(
    tx: &mut Transaction<'_, Postgres>,
    table: LiveTable,
    workspace_id: &str,
) -> GraphResult<Vec<i64>> {
    let result = match table {
        LiveTable::Nodes => {
            sqlx::query_scalar::<_, i64>(
                r#"
                SELECT valid_from FROM graph.nodes
                WHERE workspace_id = $1 AND valid_to IS NULL
                FOR UPDATE
                "#,
            )
            .bind(workspace_id)
            .fetch_all(&mut **tx)
            .await
        }
        LiveTable::Relations => {
            sqlx::query_scalar::<_, i64>(
                r#"
                SELECT valid_from FROM graph.relations
                WHERE workspace_id = $1 AND valid_to IS NULL
                FOR UPDATE
                "#,
            )
            .bind(workspace_id)
            .fetch_all(&mut **tx)
            .await
        }
        LiveTable::Claims => {
            sqlx::query_scalar::<_, i64>(
                r#"
                SELECT valid_from FROM graph.claims
                WHERE workspace_id = $1 AND valid_to IS NULL
                FOR UPDATE
                "#,
            )
            .bind(workspace_id)
            .fetch_all(&mut **tx)
            .await
        }
    };
    result.map_err(|e| map_write(e, table.name()))
}

async fn close_all_live_rows(
    tx: &mut Transaction<'_, Postgres>,
    table: LiveTable,
    workspace_id: &str,
    valid_to: i64,
    superseded_by: &str,
    now: i64,
) -> GraphResult<()> {
    let result = match table {
        LiveTable::Nodes => {
            sqlx::query(
                r#"
                UPDATE graph.nodes
                SET valid_to = $2, superseded_by = $3, updated_at = $4
                WHERE workspace_id = $1 AND valid_to IS NULL
                "#,
            )
            .bind(workspace_id)
            .bind(valid_to)
            .bind(superseded_by)
            .bind(now)
            .execute(&mut **tx)
            .await
        }
        LiveTable::Relations => {
            sqlx::query(
                r#"
                UPDATE graph.relations
                SET valid_to = $2, superseded_by = $3, updated_at = $4
                WHERE workspace_id = $1 AND valid_to IS NULL
                "#,
            )
            .bind(workspace_id)
            .bind(valid_to)
            .bind(superseded_by)
            .bind(now)
            .execute(&mut **tx)
            .await
        }
        LiveTable::Claims => {
            sqlx::query(
                r#"
                UPDATE graph.claims
                SET valid_to = $2, superseded_by = $3, updated_at = $4
                WHERE workspace_id = $1 AND valid_to IS NULL
                "#,
            )
            .bind(workspace_id)
            .bind(valid_to)
            .bind(superseded_by)
            .bind(now)
            .execute(&mut **tx)
            .await
        }
    };
    result.map(|_| ()).map_err(|e| map_write(e, table.name()))
}

async fn insert_node_live(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: &str,
    input: &UpsertNode,
    version_id: String,
    now: i64,
) -> GraphResult<NodeRow> {
    sqlx::query_as::<_, NodeRow>(
        r#"
        INSERT INTO graph.nodes (
          version_id, workspace_id, logical_id, kind, label, scope,
          aliases, evidence_ids, source_ids, confidence,
          created_by_event_id, valid_from, valid_to, superseded_by, updated_at
        )
        VALUES (
          $1, $2, $3, $4, $5, $6,
          $7, $8, $9, $10,
          $11, $12, NULL, NULL, $13
        )
        RETURNING version_id, workspace_id, logical_id, kind, label, scope,
                  aliases, evidence_ids, source_ids, confidence,
                  created_by_event_id, valid_from, valid_to, superseded_by, updated_at
        "#,
    )
    .bind(&version_id)
    .bind(workspace_id)
    .bind(&input.logical_id)
    .bind(&input.kind)
    .bind(&input.label)
    .bind(&input.scope)
    .bind(&input.aliases)
    .bind(&input.evidence_ids)
    .bind(&input.source_ids)
    .bind(input.confidence)
    .bind(&input.created_by_event_id)
    .bind(input.valid_from)
    .bind(now)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| map_write(e, "node"))
}

async fn insert_relation_live(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: &str,
    input: &UpsertRelation,
    version_id: String,
    now: i64,
) -> GraphResult<RelationRow> {
    sqlx::query_as::<_, RelationRow>(
        r#"
        INSERT INTO graph.relations (
          version_id, workspace_id, logical_id, kind,
          source_logical_id, target_logical_id, label, evidence_ids, confidence,
          created_by_event_id, valid_from, valid_to, superseded_by, updated_at
        )
        VALUES (
          $1, $2, $3, $4,
          $5, $6, $7, $8, $9,
          $10, $11, NULL, NULL, $12
        )
        RETURNING version_id, workspace_id, logical_id, kind,
                  source_logical_id, target_logical_id, label, evidence_ids, confidence,
                  created_by_event_id, valid_from, valid_to, superseded_by, updated_at
        "#,
    )
    .bind(&version_id)
    .bind(workspace_id)
    .bind(&input.logical_id)
    .bind(&input.kind)
    .bind(&input.source_logical_id)
    .bind(&input.target_logical_id)
    .bind(&input.label)
    .bind(&input.evidence_ids)
    .bind(input.confidence)
    .bind(&input.created_by_event_id)
    .bind(input.valid_from)
    .bind(now)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| map_write(e, "relation"))
}

async fn insert_claim_live(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: &str,
    input: &UpsertClaim,
    version_id: String,
    now: i64,
) -> GraphResult<ClaimRow> {
    sqlx::query_as::<_, ClaimRow>(
        r#"
        INSERT INTO graph.claims (
          version_id, workspace_id, logical_id, statement, status,
          topic_refs, source_refs, evidence_refs,
          created_by_event_id, valid_from, valid_to, superseded_by, updated_at
        )
        VALUES (
          $1, $2, $3, $4, $5,
          $6, $7, $8,
          $9, $10, NULL, NULL, $11
        )
        RETURNING version_id, workspace_id, logical_id, statement, status,
                  topic_refs, source_refs, evidence_refs,
                  created_by_event_id, valid_from, valid_to, superseded_by, updated_at
        "#,
    )
    .bind(&version_id)
    .bind(workspace_id)
    .bind(&input.logical_id)
    .bind(&input.statement)
    .bind(&input.status)
    .bind(&input.topic_refs)
    .bind(&input.source_refs)
    .bind(&input.evidence_refs)
    .bind(&input.created_by_event_id)
    .bind(input.valid_from)
    .bind(now)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| map_write(e, "claim"))
}

fn supersede_marker(created_by_event_id: Option<&str>, version_id: &str) -> String {
    // Prefer durable event id; fall back to the new version id as lineage marker.
    created_by_event_id
        .map(str::to_string)
        .unwrap_or_else(|| version_id.to_string())
}

fn new_version_id(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::now_v7().simple())
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn map_read(error: sqlx::Error) -> GraphError {
    tracing::warn!(%error, "graph database read failed");
    GraphError::Internal("graph database read failed".into())
}

fn map_tx(error: sqlx::Error) -> GraphError {
    tracing::warn!(%error, "graph database transaction failed");
    GraphError::Internal("graph database transaction failed".into())
}

fn map_write(error: sqlx::Error, entity: &str) -> GraphError {
    if let Some(database_error) = error.as_database_error() {
        return match database_error.code().as_deref() {
            Some("23503") => GraphError::Conflict(format!("invalid {entity} relationship")),
            Some("23505") => GraphError::Conflict(format!("{entity} already exists")),
            _ => {
                tracing::warn!(%error, entity, "graph database write failed");
                GraphError::Internal("graph database write failed".into())
            }
        };
    }
    tracing::warn!(%error, entity, "graph database write failed");
    GraphError::Internal("graph database write failed".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn require_database_url() -> String {
        std::env::var("ETYMA_DATABASE_URL")
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .expect("ETYMA_DATABASE_URL required for ignored Postgres tests")
    }

    async fn create_workspace(pool: &sqlx::PgPool, suffix: &str) -> String {
        let org_id = format!("org_{suffix}");
        let workspace_id = format!("ws_{suffix}");
        let now = 1_i64;
        sqlx::query("INSERT INTO control.orgs (id, name, created_at) VALUES ($1, $2, $3)")
            .bind(&org_id)
            .bind("Graph test")
            .bind(now)
            .execute(pool)
            .await
            .expect("insert org");
        sqlx::query("INSERT INTO control.workspaces (id, org_id, created_at) VALUES ($1, $2, $3)")
            .bind(&workspace_id)
            .bind(&org_id)
            .bind(now)
            .execute(pool)
            .await
            .expect("insert workspace");
        workspace_id
    }

    #[tokio::test]
    #[ignore = "requires ETYMA_DATABASE_URL"]
    async fn live_projection_round_trips_and_supersedes() {
        let pool = crate::db::connect_and_migrate(&require_database_url())
            .await
            .expect("connect and migrate");
        let suffix = uuid::Uuid::now_v7().simple().to_string();
        let workspace_a = create_workspace(&pool, &format!("{suffix}_a")).await;
        let workspace_b = create_workspace(&pool, &format!("{suffix}_b")).await;
        let store = GraphStore::new(pool);

        let n1 = store
            .upsert_live_node(
                &workspace_a,
                &UpsertNode {
                    logical_id: "node_auth".into(),
                    kind: "concept".into(),
                    label: "Auth".into(),
                    scope: "workspace".into(),
                    aliases: vec!["authentication".into()],
                    evidence_ids: vec!["ev_1".into()],
                    source_ids: vec!["src_1".into()],
                    confidence: Some(0.9),
                    created_by_event_id: Some("evt_1".into()),
                    valid_from: 1,
                },
            )
            .await
            .expect("insert node");
        assert!(n1.valid_to.is_none());
        assert_eq!(n1.logical_id, "node_auth");

        let n1b = store
            .upsert_live_node(
                &workspace_a,
                &UpsertNode {
                    logical_id: "node_auth".into(),
                    kind: "concept".into(),
                    label: "Authentication".into(),
                    scope: "workspace".into(),
                    aliases: vec!["auth".into()],
                    evidence_ids: vec!["ev_1".into(), "ev_2".into()],
                    source_ids: vec!["src_1".into()],
                    confidence: Some(0.95),
                    created_by_event_id: Some("evt_2".into()),
                    valid_from: 2,
                },
            )
            .await
            .expect("supersede node");
        assert_eq!(n1b.label, "Authentication");
        assert_ne!(n1b.version_id, n1.version_id);

        let non_monotonic = store
            .upsert_live_node(
                &workspace_a,
                &UpsertNode {
                    logical_id: "node_auth".into(),
                    kind: "concept".into(),
                    label: "Stale".into(),
                    scope: "workspace".into(),
                    aliases: vec![],
                    evidence_ids: vec![],
                    source_ids: vec![],
                    confidence: None,
                    created_by_event_id: Some("evt_stale".into()),
                    valid_from: 2,
                },
            )
            .await;
        assert!(matches!(non_monotonic, Err(GraphError::Conflict(_))));

        let snap_a = store.live_snapshot(&workspace_a).await.expect("snapshot A");
        assert_eq!(snap_a.nodes.len(), 1);
        assert_eq!(snap_a.nodes[0].label, "Authentication");
        assert_eq!(snap_a.nodes[0].evidence_ids, vec!["ev_1", "ev_2"]);

        let snap_b = store
            .live_snapshot(&workspace_b)
            .await
            .expect("snapshot B empty");
        assert!(snap_b.nodes.is_empty());

        store
            .upsert_live_relation(
                &workspace_a,
                &UpsertRelation {
                    logical_id: "rel_auth_token".into(),
                    kind: "related_to".into(),
                    source_logical_id: "node_auth".into(),
                    target_logical_id: "node_token".into(),
                    label: "issues".into(),
                    evidence_ids: vec!["ev_1".into()],
                    confidence: None,
                    created_by_event_id: Some("evt_3".into()),
                    valid_from: 3,
                },
            )
            .await
            .expect("insert relation");

        store
            .upsert_live_node(
                &workspace_a,
                &UpsertNode {
                    logical_id: "node_token".into(),
                    kind: "concept".into(),
                    label: "Token".into(),
                    scope: "workspace".into(),
                    aliases: vec![],
                    evidence_ids: vec!["ev_1".into()],
                    source_ids: vec!["src_1".into()],
                    confidence: None,
                    created_by_event_id: Some("evt_3".into()),
                    valid_from: 3,
                },
            )
            .await
            .expect("token node");

        store
            .upsert_live_claim(
                &workspace_a,
                &UpsertClaim {
                    logical_id: "claim_1".into(),
                    statement: "Tokens are workspace-scoped".into(),
                    status: "supported".into(),
                    topic_refs: vec!["node_token".into()],
                    source_refs: vec!["src_1".into()],
                    evidence_refs: vec!["ev_1".into()],
                    created_by_event_id: Some("evt_3".into()),
                    valid_from: 3,
                },
            )
            .await
            .expect("claim");

        let snap = store.live_snapshot(&workspace_a).await.expect("full snap");
        assert_eq!(snap.nodes.len(), 2);
        assert_eq!(snap.relations.len(), 1);
        assert_eq!(snap.claims.len(), 1);

        let batch = store
            .live_nodes_for_evidence_ids(&workspace_a, &["ev_1".into(), "ev_2".into()])
            .await
            .expect("batch evidence");
        assert_eq!(batch.len(), 2);

        let linked = store
            .live_nodes_for_evidence(&workspace_a, "ev_1")
            .await
            .expect("nodes for evidence");
        assert_eq!(linked.len(), 2);

        let node_ids: Vec<String> = linked.iter().map(|n| n.logical_id.clone()).collect();
        let adjacent = store
            .live_relations_touching(&workspace_a, &node_ids)
            .await
            .expect("adjacent");
        assert_eq!(adjacent.len(), 1);
        assert_eq!(adjacent[0].logical_id, "rel_auth_token");

        let history_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)::bigint FROM graph.nodes
            WHERE workspace_id = $1 AND logical_id = 'node_auth'
            "#,
        )
        .bind(&workspace_a)
        .fetch_one(&store.pool)
        .await
        .expect("history count");
        assert_eq!(history_count, 2);

        // replace_live_projection closes prior live set atomically.
        let replaced = store
            .replace_live_projection(
                &workspace_a,
                10,
                "evt_replace",
                &[UpsertNode {
                    logical_id: "node_only".into(),
                    kind: "concept".into(),
                    label: "Only".into(),
                    scope: "workspace".into(),
                    aliases: vec![],
                    evidence_ids: vec!["ev_9".into()],
                    source_ids: vec![],
                    confidence: None,
                    created_by_event_id: None,
                    valid_from: 10,
                }],
                &[],
                &[],
            )
            .await
            .expect("replace");
        assert_eq!(replaced.nodes.len(), 1);
        assert_eq!(replaced.nodes[0].logical_id, "node_only");
        assert!(replaced.relations.is_empty());
        assert!(replaced.claims.is_empty());

        let wire = GraphSnapshotResponse::from(replaced);
        assert_eq!(wire.projection, "live");
        assert_eq!(wire.store, "postgres.graph");
        assert_eq!(wire.nodes[0].id, "node_only");
    }

    #[tokio::test]
    #[ignore = "requires ETYMA_DATABASE_URL"]
    async fn cross_workspace_write_requires_workspace_fk() {
        let pool = crate::db::connect_and_migrate(&require_database_url())
            .await
            .expect("connect and migrate");
        let store = GraphStore::new(pool);
        let err = store
            .upsert_live_node(
                "ws_missing",
                &UpsertNode {
                    logical_id: "n1".into(),
                    kind: "concept".into(),
                    label: "X".into(),
                    scope: "workspace".into(),
                    aliases: vec![],
                    evidence_ids: vec![],
                    source_ids: vec![],
                    confidence: None,
                    created_by_event_id: None,
                    valid_from: 1,
                },
            )
            .await
            .expect_err("missing workspace FK");
        assert!(
            matches!(err, GraphError::Conflict(_)) || matches!(err, GraphError::Internal(_)),
            "unexpected err: {err}"
        );
    }
}
