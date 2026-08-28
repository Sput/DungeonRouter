use axum::{
    Json, Router,
    extract::State,
    http::{HeaderValue, Method, StatusCode},
    routing::{get, post},
};
use routing::{
    CompletionRequest, CompletionResponse, ModelDescriptor, ModelTier, RouterError,
    SharedModelRouter, model_catalog,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tower_http::{
    cors::CorsLayer,
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
};
use tracing::Level;

pub mod routing;

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub router: SharedModelRouter,
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
        .route("/api/models", get(models))
        .route("/api/router/complete", post(complete))
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

async fn models() -> Json<Vec<ModelDescriptor>> {
    Json(model_catalog())
}

#[derive(Debug, Deserialize)]
struct ManualCompletionRequest {
    prompt: String,
    model: ModelTier,
    max_output_tokens: Option<u32>,
}

async fn complete(
    State(state): State<AppState>,
    Json(request): Json<ManualCompletionRequest>,
) -> Result<Json<CompletionResponse>, RouterApiError> {
    let prompt = request.prompt.trim();
    if prompt.is_empty() {
        return Err(RouterApiError::InvalidPrompt(
            "prompt must not be empty".into(),
        ));
    }
    if prompt.chars().count() > 12_000 {
        return Err(RouterApiError::InvalidPrompt(
            "prompt must not exceed 12,000 characters".into(),
        ));
    }
    let max_output_tokens = request.max_output_tokens.unwrap_or(800);
    if !(1..=4_000).contains(&max_output_tokens) {
        return Err(RouterApiError::InvalidPrompt(
            "max_output_tokens must be between 1 and 4,000".into(),
        ));
    }

    let response = state
        .router
        .complete(CompletionRequest {
            prompt: prompt.to_owned(),
            model: request.model,
            max_output_tokens,
        })
        .await?;
    Ok(Json(response))
}

enum RouterApiError {
    InvalidPrompt(String),
    Router(RouterError),
}

impl From<RouterError> for RouterApiError {
    fn from(value: RouterError) -> Self {
        Self::Router(value)
    }
}

impl axum::response::IntoResponse for RouterApiError {
    fn into_response(self) -> axum::response::Response {
        match self {
            Self::InvalidPrompt(error) => {
                (StatusCode::BAD_REQUEST, Json(ApiErrorBody { error })).into_response()
            }
            Self::Router(error) => error.into_response(),
        }
    }
}

#[derive(Serialize)]
struct ApiErrorBody {
    error: String,
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
    use crate::routing::{CompletionResponse, ModelTier, testing::MockRouter};
    use axum::{
        body::Body,
        http::{Request, header},
    };
    use http_body_util::BodyExt;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::sync::Arc;
    use tower::ServiceExt;

    async fn test_state(router: SharedModelRouter) -> AppState {
        let db = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .expect("in-memory database should open");
        AppState { db, router }
    }

    fn mock_router() -> Arc<MockRouter> {
        Arc::new(MockRouter::new(CompletionResponse {
            content: "Mock ruling".into(),
            selected_model: ModelTier::Mini,
            upstream_model: "gpt-5-mini".into(),
            usage: None,
        }))
    }

    #[tokio::test]
    async fn health_reports_connected_database() {
        let response = app(test_state(mock_router()).await)
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

    #[tokio::test]
    async fn model_catalog_exposes_all_manual_tiers() {
        let response = app(test_state(mock_router()).await)
            .oneshot(
                Request::builder()
                    .uri("/api/models")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("model request should succeed");

        assert!(response.status().is_success());
        let body = response
            .into_body()
            .collect()
            .await
            .expect("body should collect")
            .to_bytes();
        let text = String::from_utf8(body.to_vec()).expect("body should be UTF-8");
        assert!(text.contains("gpt-5-nano"));
        assert!(text.contains("gpt-5-mini"));
        assert!(text.contains("\"model_id\":\"gpt-5\""));
    }

    #[tokio::test]
    async fn manual_completion_forwards_selected_tier_to_router() {
        let router = mock_router();
        let response = app(test_state(router.clone()).await)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/router/complete")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"prompt":"Explain prone","model":"mini","max_output_tokens":500}"#,
                    ))
                    .expect("request should build"),
            )
            .await
            .expect("completion request should succeed");

        assert!(response.status().is_success());
        let calls = router.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].model, ModelTier::Mini);
        assert_eq!(calls[0].prompt, "Explain prone");
        assert_eq!(calls[0].max_output_tokens, 500);
    }

    #[tokio::test]
    async fn empty_prompt_is_rejected_without_calling_router() {
        let router = mock_router();
        let response = app(test_state(router.clone()).await)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/router/complete")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"prompt":"  ","model":"nano"}"#))
                    .expect("request should build"),
            )
            .await
            .expect("completion request should finish");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(router.calls().is_empty());
    }
}
