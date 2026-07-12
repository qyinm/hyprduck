//! Cloud graph plane store (S-PG4).
//!
//! Relational projection of nodes / relations / claims in Postgres.
//! GraphQLite remains local/engine-only and is never opened by etyma-server.
//! Full engine materialize wiring is S7 (PON-11); this module is the durable
//! cloud write/read boundary materialize will target.

use sqlx::PgPool;
use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
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

#[derive(Debug, Clone, sqlx::FromRow)]
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

#[derive(Debug, Clone, sqlx::FromRow)]
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
    NotFound { entity: &'static str, id: String },
    Conflict(String),
    Internal(String),
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { entity, id } => write!(f, "{entity} not found: {id}"),
            Self::Conflict(message) | Self::Internal(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for GraphError {}

impl From<GraphError> for crate::store::StoreError {
    fn from(error: GraphError) -> Self {
        match error {
            GraphError::NotFound { entity, id } => Self::NotFound { entity, id },
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
        let mut tx = self.pool.begin().await.map_err(map_write_tx)?;
        let version_id = new_version_id("nv");
        let now = unix_now();
        let supersede_event = input
            .created_by_event_id
            .clone()
            .unwrap_or_else(|| version_id.clone());

        sqlx::query(
            r#"
            UPDATE graph.nodes
            SET valid_to = $3,
                superseded_by = $4,
                updated_at = $5
            WHERE workspace_id = $1
              AND logical_id = $2
              AND valid_to IS NULL
            "#,
        )
        .bind(workspace_id)
        .bind(&input.logical_id)
        .bind(input.valid_from)
        .bind(&supersede_event)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| map_write(e, "node"))?;

        let row = sqlx::query_as::<_, NodeRow>(
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
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| map_write(e, "node"))?;

        tx.commit().await.map_err(map_write_tx)?;
        Ok(row)
    }

    pub async fn upsert_live_relation(
        &self,
        workspace_id: &str,
        input: &UpsertRelation,
    ) -> GraphResult<RelationRow> {
        let mut tx = self.pool.begin().await.map_err(map_write_tx)?;
        let version_id = new_version_id("rv");
        let now = unix_now();
        let supersede_event = input
            .created_by_event_id
            .clone()
            .unwrap_or_else(|| version_id.clone());

        sqlx::query(
            r#"
            UPDATE graph.relations
            SET valid_to = $3,
                superseded_by = $4,
                updated_at = $5
            WHERE workspace_id = $1
              AND logical_id = $2
              AND valid_to IS NULL
            "#,
        )
        .bind(workspace_id)
        .bind(&input.logical_id)
        .bind(input.valid_from)
        .bind(&supersede_event)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| map_write(e, "relation"))?;

        let row = sqlx::query_as::<_, RelationRow>(
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
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| map_write(e, "relation"))?;

        tx.commit().await.map_err(map_write_tx)?;
        Ok(row)
    }

    pub async fn upsert_live_claim(
        &self,
        workspace_id: &str,
        input: &UpsertClaim,
    ) -> GraphResult<ClaimRow> {
        let mut tx = self.pool.begin().await.map_err(map_write_tx)?;
        let version_id = new_version_id("cv");
        let now = unix_now();
        let supersede_event = input
            .created_by_event_id
            .clone()
            .unwrap_or_else(|| version_id.clone());

        sqlx::query(
            r#"
            UPDATE graph.claims
            SET valid_to = $3,
                superseded_by = $4,
                updated_at = $5
            WHERE workspace_id = $1
              AND logical_id = $2
              AND valid_to IS NULL
            "#,
        )
        .bind(workspace_id)
        .bind(&input.logical_id)
        .bind(input.valid_from)
        .bind(&supersede_event)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| map_write(e, "claim"))?;

        let row = sqlx::query_as::<_, ClaimRow>(
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
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| map_write(e, "claim"))?;

        tx.commit().await.map_err(map_write_tx)?;
        Ok(row)
    }

    /// Live projection only (`valid_to IS NULL`). No GraphQLite open.
    pub async fn live_snapshot(&self, workspace_id: &str) -> GraphResult<GraphSnapshot> {
        let nodes = sqlx::query_as::<_, NodeRow>(
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
        .fetch_all(&self.pool)
        .await
        .map_err(map_read)?;

        let relations = sqlx::query_as::<_, RelationRow>(
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
        .fetch_all(&self.pool)
        .await
        .map_err(map_read)?;

        let claims = sqlx::query_as::<_, ClaimRow>(
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
        .fetch_all(&self.pool)
        .await
        .map_err(map_read)?;

        Ok(GraphSnapshot {
            workspace_id: workspace_id.to_string(),
            nodes,
            relations,
            claims,
        })
    }

    /// Live nodes that cite the given evidence id (for pack graph trails).
    pub async fn live_nodes_for_evidence(
        &self,
        workspace_id: &str,
        evidence_id: &str,
    ) -> GraphResult<Vec<NodeRow>> {
        sqlx::query_as::<_, NodeRow>(
            r#"
            SELECT version_id, workspace_id, logical_id, kind, label, scope,
                   aliases, evidence_ids, source_ids, confidence,
                   created_by_event_id, valid_from, valid_to, superseded_by, updated_at
            FROM graph.nodes
            WHERE workspace_id = $1
              AND valid_to IS NULL
              AND $2 = ANY(evidence_ids)
            ORDER BY logical_id
            "#,
        )
        .bind(workspace_id)
        .bind(evidence_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_read)
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

fn map_write_tx(error: sqlx::Error) -> GraphError {
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

        let snap_a = store
            .live_snapshot(&workspace_a)
            .await
            .expect("snapshot A");
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

        // History retained: superseded row still present but not live.
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
