//! Postgres pool connect, migrations, and lightweight health probe.
//!
//! Plane schemas only in S-PG1; product tables arrive in S-PG2+.

use anyhow::{Context, Result};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

/// Connect to Postgres and apply embedded migrations.
///
/// Safe to call more than once against the same database (sqlx tracks applied
/// versions; schema DDL uses `IF NOT EXISTS`).
pub async fn connect_and_migrate(database_url: &str) -> Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
        .context("connect postgres")?;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("run migrations")?;
    Ok(pool)
}

/// Cheap liveness probe (`SELECT 1`) for health endpoints and tests.
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
    use std::collections::HashSet;

    fn database_url() -> Option<String> {
        std::env::var("ETYMA_DATABASE_URL")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    #[tokio::test]
    async fn connect_and_migrate_is_idempotent_and_creates_plane_schemas() {
        let Some(url) = database_url() else {
            eprintln!("skipping: ETYMA_DATABASE_URL unset");
            return;
        };

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

        let found: HashSet<&str> = schemas.iter().map(String::as_str).collect();
        assert!(
            found.contains("control") && found.contains("knowledge") && found.contains("graph"),
            "expected control/knowledge/graph schemas, got {schemas:?}"
        );

        // No product tables in these schemas yet (S-PG2/3/4).
        let product_tables: Vec<String> = sqlx::query_scalar(
            r#"
            SELECT table_schema || '.' || table_name
            FROM information_schema.tables
            WHERE table_schema IN ('control', 'knowledge', 'graph')
              AND table_type = 'BASE TABLE'
            ORDER BY 1
            "#,
        )
        .fetch_all(&pool2)
        .await
        .expect("list product tables in plane schemas");
        assert!(
            product_tables.is_empty(),
            "S-PG1 must not create product tables; found {product_tables:?}"
        );
    }
}
