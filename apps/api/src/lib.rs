use axum::{
    Json, Router,
    extract::State,
    http::{HeaderValue, Method},
    routing::get,
};
use serde::Serialize;
use sqlx::SqlitePool;
use tower_http::{
    cors::CorsLayer,
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
};
use tracing::Level;

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub service: &'static str,
    pub status: &'static str,
    pub database: &'static str,
}

pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .with_state(state)
        .layer(
            CorsLayer::new()
                .allow_origin(HeaderValue::from_static("http://localhost:5173"))
                .allow_methods([Method::GET, Method::POST, Method::DELETE])
                .allow_headers(tower_http::cors::Any),
        )
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    let database = if sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(&state.db)
        .await
        .is_ok()
    {
        "connected"
    } else {
        "unavailable"
    };

    Json(HealthResponse {
        service: "dungeon-router-api",
        status: if database == "connected" {
            "ok"
        } else {
            "degraded"
        },
        database,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use sqlx::sqlite::SqlitePoolOptions;
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_reports_connected_database() {
        let db = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .expect("in-memory database should open");
        let response = app(AppState { db })
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("health request should succeed");

        assert!(response.status().is_success());
        let body = response
            .into_body()
            .collect()
            .await
            .expect("body should collect")
            .to_bytes();
        let text = String::from_utf8(body.to_vec()).expect("body should be UTF-8");
        assert!(text.contains("\"database\":\"connected\""));
    }
}
