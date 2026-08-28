use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderValue, Method, StatusCode},
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
};
use futures_core::Stream;
use futures_util::StreamExt;
use routing::{
    CompletionRequest, CompletionResponse, ModelDescriptor, ModelTier, RouterError,
    SharedModelRouter, StreamEvent, model_catalog,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tower_http::{
    cors::CorsLayer,
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
};
use tracing::Level;

pub mod routing;
pub mod search;
pub mod srd;

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
        .route("/api/search", get(search_rules))
        .route("/api/sources/{chunk_id}", get(source_passage))
        .route("/api/router/complete", post(complete))
        .route("/api/router/stream", post(stream))
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
struct SearchQuery {
    q: String,
    limit: Option<u32>,
}

#[derive(Debug, Serialize)]
struct SearchResponse {
    query: String,
    results: Vec<search::SearchResult>,
}

async fn search_rules(
    State(state): State<AppState>,
    Query(request): Query<SearchQuery>,
) -> Result<Json<SearchResponse>, SearchApiError> {
    let query = request.q.trim();
    if query.chars().count() > 500 {
        return Err(SearchApiError::Invalid(
            "search query must not exceed 500 characters".into(),
        ));
    }
    let limit = request.limit.unwrap_or(6);
    if !(1..=20).contains(&limit) {
        return Err(SearchApiError::Invalid(
            "limit must be between 1 and 20".into(),
        ));
    }
    let results = search::search(&state.db, query, limit).await?;
    Ok(Json(SearchResponse {
        query: query.to_owned(),
        results,
    }))
}

async fn source_passage(
    State(state): State<AppState>,
    Path(chunk_id): Path<i64>,
) -> Result<Json<search::SourceChunk>, SearchApiError> {
    Ok(Json(search::source(&state.db, chunk_id).await?))
}

enum SearchApiError {
    Invalid(String),
    Search(search::SearchError),
}

impl From<search::SearchError> for SearchApiError {
    fn from(value: search::SearchError) -> Self {
        match value {
            search::SearchError::EmptyQuery => Self::Invalid(value.to_string()),
            _ => Self::Search(value),
        }
    }
}

impl axum::response::IntoResponse for SearchApiError {
    fn into_response(self) -> axum::response::Response {
        let (status, error) = match self {
            Self::Invalid(error) => (StatusCode::BAD_REQUEST, error),
            Self::Search(search::SearchError::NotFound) => (
                StatusCode::NOT_FOUND,
                search::SearchError::NotFound.to_string(),
            ),
            Self::Search(error) => (StatusCode::SERVICE_UNAVAILABLE, error.to_string()),
        };
        (status, Json(ApiErrorBody { error })).into_response()
    }
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
    let request = validate_request(request)?;
    let response = state.router.complete(request).await?;
    Ok(Json(response))
}

async fn stream(
    State(state): State<AppState>,
    Json(request): Json<ManualCompletionRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, RouterApiError> {
    let stream = state.router.stream(validate_request(request)?).await?;
    let events = stream.map(|result| {
        let event = match result {
            Ok(event) => stream_event(event),
            Err(error) => Event::default()
                .event("error")
                .json_data(ApiErrorBody {
                    error: error.to_string(),
                })
                .expect("error event should serialize"),
        };
        Ok(event)
    });
    Ok(Sse::new(events).keep_alive(KeepAlive::default()))
}

fn stream_event(event: StreamEvent) -> Event {
    let name = match event {
        StreamEvent::Metadata { .. } => "metadata",
        StreamEvent::Delta { .. } => "delta",
        StreamEvent::Usage { .. } => "usage",
        StreamEvent::Done => "done",
    };
    Event::default()
        .event(name)
        .json_data(event)
        .expect("stream event should serialize")
}

fn validate_request(request: ManualCompletionRequest) -> Result<CompletionRequest, RouterApiError> {
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
    Ok(CompletionRequest {
        prompt: prompt.to_owned(),
        model: request.model,
        max_output_tokens,
    })
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
    use crate::{
        routing::{CompletionResponse, ModelTier, testing::MockRouter},
        srd::{SrdChunk, SrdDocument, SrdSnapshot, ingest_snapshot},
    };
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

    async fn searchable_test_state(router: SharedModelRouter) -> AppState {
        let state = test_state(router).await;
        sqlx::migrate!("./migrations").run(&state.db).await.unwrap();
        ingest_snapshot(
            &state.db,
            &SrdSnapshot {
                title: "SRD".into(),
                edition: "5e 2014".into(),
                license: "CC BY 4.0".into(),
                upstream: "example".into(),
                revision: "fixture".into(),
                documents: vec![SrdDocument {
                    title: "Conditions".into(),
                    source_path: "08_Gamemastering/Conditions.md".into(),
                    chunks: vec![SrdChunk {
                        heading: "Prone".into(),
                        section_path: "Conditions > Prone".into(),
                        content: "A prone creature's only movement option is to crawl.".into(),
                        ordinal: 0,
                        source_locator: "08_Gamemastering/Conditions.md#prone".into(),
                    }],
                }],
            },
        )
        .await
        .unwrap();
        state
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

    #[tokio::test]
    async fn streaming_completion_emits_metadata_delta_and_done_events() {
        let router = mock_router();
        let response = app(test_state(router.clone()).await)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/router/stream")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"prompt":"Explain prone","model":"mini","max_output_tokens":500}"#,
                    ))
                    .expect("request should build"),
            )
            .await
            .expect("stream request should succeed");

        assert!(response.status().is_success());
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/event-stream"
        );
        let body = response
            .into_body()
            .collect()
            .await
            .expect("stream should collect")
            .to_bytes();
        let text = String::from_utf8(body.to_vec()).expect("body should be UTF-8");
        assert!(text.contains("event: metadata"));
        assert!(text.contains("event: delta"));
        assert!(text.contains("Mock ruling"));
        assert!(text.contains("event: done"));
        assert_eq!(router.calls().len(), 1);
    }

    #[tokio::test]
    async fn rules_search_returns_citable_results() {
        let response = app(searchable_test_state(mock_router()).await)
            .oneshot(
                Request::builder()
                    .uri("/api/search?q=what%20does%20prone%20do")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let text = String::from_utf8(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap();
        assert!(text.contains("Conditions &gt; Prone") || text.contains("Conditions > Prone"));
        assert!(text.contains("Conditions.md#prone"));
        assert!(text.contains("fixture"));
    }

    #[tokio::test]
    async fn source_endpoint_returns_full_passage() {
        let state = searchable_test_state(mock_router()).await;
        let chunk_id: i64 = sqlx::query_scalar("SELECT id FROM source_chunks LIMIT 1")
            .fetch_one(&state.db)
            .await
            .unwrap();
        let response = app(state)
            .oneshot(
                Request::builder()
                    .uri(format!("/api/sources/{chunk_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let text = String::from_utf8(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap();
        assert!(text.contains("only movement option"));
    }
}
