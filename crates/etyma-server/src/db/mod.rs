//! Postgres pool connect, migrations, and lightweight health probe.
//!
//! S-PG1: plane schemas. S-PG2: control product tables. knowledge/graph tables in S-PG3/4.

use anyhow::{Context, Result};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::time::Duration;

const DEFAULT_PG_MAX_CONNECTIONS: u32 = 5;
const DEFAULT_PG_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);

/// Connect to Postgres and apply embedded migrations.
///
/// Safe to call more than once against the same database (sqlx tracks applied
/// versions; schema DDL uses `IF NOT EXISTS`).
pub async fn connect_and_migrate(database_url: &str) -> Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(DEFAULT_PG_MAX_CONNECTIONS)
        .acquire_timeout(DEFAULT_PG_ACQUIRE_TIMEOUT)
        .connect(database_url)
        .await
        .context("connect postgres")?;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("run migrations")?;
    Ok(pool)
}

/// Cheap readiness probe (`SELECT 1`) for health endpoints and tests.
pub async fn health_check(pool: &PgPool) -> Result<()> {
    sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(pool)
        .await
        .context("postgres health check (SELECT 1)")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn require_database_url() -> String {
        std::env::var("ETYMA_DATABASE_URL")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .expect(
                "ETYMA_DATABASE_URL required for ignored Postgres tests \
                 (run: docker compose up -d && cargo test -p etyma-server -- --include-ignored)",
            )
    }

    #[tokio::test]
    #[ignore = "requires ETYMA_DATABASE_URL"]
    async fn connect_and_migrate_is_idempotent_and_creates_plane_schemas() {
        let url = require_database_url();

        let pool = connect_and_migrate(&url)
            .await
            .expect("first connect_and_migrate");
        health_check(&pool).await.expect("health after first migrate");

        let pool2 = connect_and_migrate(&url)
            .await
            .expect("second connect_and_migrate (idempotent)");
        health_check(&pool2)
            .await
            .expect("health after second migrate");

        let schemas: Vec<String> = sqlx::query_scalar(
            r#"
            SELECT schema_name
            FROM information_schema.schemata
            WHERE schema_name IN ('control', 'knowledge', 'graph')
            ORDER BY schema_name
            "#,
        )
        .fetch_all(&pool2)
        .await
        .expect("list plane schemas");

        assert_eq!(
            schemas,
            vec!["control", "graph", "knowledge"],
            "expected ordered plane schemas"
        );

        // Control product tables from S-PG2.
        let control_tables: Vec<String> = sqlx::query_scalar(
            r#"
            SELECT table_name
            FROM information_schema.tables
            WHERE table_schema = 'control'
              AND table_type = 'BASE TABLE'
            ORDER BY table_name
            "#,
        )
        .fetch_all(&pool2)
        .await
        .expect("list control product tables");
        assert_eq!(
            control_tables,
            vec![
                "api_tokens",
                "audit_events",
                "memberships",
                "orgs",
                "users",
                "workspaces",
            ],
            "expected S-PG2 control product tables"
        );

        // knowledge/graph still have no product tables (S-PG3/4).
        let other_plane_tables: Vec<String> = sqlx::query_scalar(
            r#"
            SELECT table_schema || '.' || table_name
            FROM information_schema.tables
            WHERE table_schema IN ('knowledge', 'graph')
              AND table_type = 'BASE TABLE'
            ORDER BY 1
            "#,
        )
        .fetch_all(&pool2)
        .await
        .expect("list knowledge/graph product tables");
        assert!(
            other_plane_tables.is_empty(),
            "knowledge/graph must not have product tables yet; found {other_plane_tables:?}"
        );
    }
}
