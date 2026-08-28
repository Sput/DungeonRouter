use std::{env, fs};

use anyhow::{Context, Result};
use dungeon_router_api::srd::{SrdSnapshot, ingest_snapshot};
use sqlx::sqlite::SqlitePoolOptions;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let snapshot_path = env::args()
        .nth(1)
        .unwrap_or_else(|| "content/srd-5.1.json".into());
    let database_url = env::var("DATABASE_URL")
        .unwrap_or_else(|_| "sqlite://data/dungeon-router.db?mode=rwc".into());
    let snapshot: SrdSnapshot = serde_json::from_slice(
        &fs::read(&snapshot_path).with_context(|| format!("failed to read {snapshot_path}"))?,
    )
    .context("snapshot JSON is invalid")?;
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    let chunks = ingest_snapshot(&pool, &snapshot).await?;
    println!(
        "Indexed {} documents and {chunks} chunks",
        snapshot.documents.len()
    );
    Ok(())
}
