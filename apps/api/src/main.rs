use std::{env, net::SocketAddr, path::Path, sync::Arc};

use anyhow::{Context, Result};
use dungeon_router_api::{
    AppState, app, auth::SupabaseAuth, routing::SwitchyardRouter, srd::ensure_bundled_srd,
    usage::CostConfig,
};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    init_tracing();

    let host = env::var("APP_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = env::var("APP_PORT")
        .unwrap_or_else(|_| "4000".into())
        .parse::<u16>()
        .context("APP_PORT must be a valid port")?;
    let database_url = env::var("DATABASE_URL")
        .unwrap_or_else(|_| "sqlite://data/dungeon-router.db?mode=rwc".into());
    let switchyard_base_url =
        env::var("SWITCHYARD_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:4100".into());
    let supabase_url = env::var("SUPABASE_URL").context("SUPABASE_URL is required")?;
    let supabase_anon_key =
        env::var("SUPABASE_ANON_KEY").context("SUPABASE_ANON_KEY is required")?;
    let auth = SupabaseAuth::new(&supabase_url, supabase_anon_key)
        .context("failed to configure Supabase authentication")?;

    if let Some(path) = sqlite_parent_path(&database_url) {
        tokio::fs::create_dir_all(path)
            .await
            .context("failed to create SQLite data directory")?;
    }

    let options: SqliteConnectOptions = database_url
        .parse()
        .context("DATABASE_URL must be a valid SQLite URL")?;
    let db = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .context("failed to connect to SQLite")?;

    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .context("failed to run database migrations")?;
    let indexed_chunks = ensure_bundled_srd(&db)
        .await
        .context("failed to initialize bundled SRD content")?;
    if indexed_chunks > 0 {
        info!(indexed_chunks, "Indexed bundled SRD content");
    }

    let router = Arc::new(
        SwitchyardRouter::new(switchyard_base_url)
            .context("failed to initialize Switchyard HTTP client")?,
    );

    let address: SocketAddr = format!("{host}:{port}")
        .parse()
        .context("APP_HOST and APP_PORT must form a valid socket address")?;
    let listener = TcpListener::bind(address)
        .await
        .context("failed to bind API listener")?;

    info!(%address, "DungeonRouter API listening");
    let costs = CostConfig::from_env().context("failed to load cost controls")?;
    axum::serve(
        listener,
        app(AppState {
            db,
            router,
            costs,
            auth: Some(auth),
        }),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .context("API server exited unexpectedly")?;

    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("dungeon_router_api=debug,tower_http=info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn sqlite_parent_path(database_url: &str) -> Option<&Path> {
    let path = database_url
        .strip_prefix("sqlite://")?
        .split('?')
        .next()
        .filter(|path| !path.is_empty() && *path != ":memory:")?;
    Path::new(path)
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::warn!(%error, "failed to listen for shutdown signal");
    }
}
