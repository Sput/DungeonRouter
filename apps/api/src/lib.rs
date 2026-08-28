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
    CompletionRequest, CompletionResponse, ModelDescriptor, ModelTier, RouterError, RoutingMode,
    SharedModelRouter, StreamEvent, model_catalog,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::time::Instant;
use tower_http::{
    cors::CorsLayer,
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
};
use tracing::Level;

pub mod chat;
pub mod notes;
pub mod routing;
pub mod search;
pub mod srd;
pub mod usage;

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub router: SharedModelRouter,
    pub costs: usage::CostConfig,
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
        .route("/api/config/public", get(public_config))
        .route("/api/models", get(models))
        .route("/api/activity", get(activity))
        .route("/api/usage/summary", get(usage_summary))
        .route("/api/usage/daily", get(daily_usage))
        .route("/api/search", get(search_rules))
        .route("/api/sources/{chunk_id}", get(source_passage))
        .route("/api/notes", get(list_notes).post(create_note))
        .route("/api/notes/{note_id}", get(get_note).delete(delete_note))
        .route("/api/router/complete", post(complete))
        .route("/api/router/stream", post(stream))
        .route("/api/chat", post(chat))
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

#[derive(Serialize)]
struct PublicConfig {
    monthly_cost_warning_usd: f64,
    monthly_cost_hard_limit_usd: f64,
    pricing_version: &'static str,
}

async fn public_config(State(state): State<AppState>) -> Json<PublicConfig> {
    Json(PublicConfig {
        monthly_cost_warning_usd: state.costs.monthly_warning_usd,
        monthly_cost_hard_limit_usd: state.costs.monthly_hard_limit_usd,
        pricing_version: usage::PRICING_VERSION,
    })
}

#[derive(Deserialize)]
struct ActivityQuery {
    limit: Option<u32>,
}

async fn activity(
    State(state): State<AppState>,
    Query(query): Query<ActivityQuery>,
) -> Result<Json<Vec<usage::ActivityItem>>, UsageApiError> {
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    Ok(Json(usage::activity(&state.db, limit).await?))
}

async fn usage_summary(
    State(state): State<AppState>,
) -> Result<Json<usage::UsageSummary>, UsageApiError> {
    Ok(Json(usage::summary(&state.db, &state.costs).await?))
}

async fn daily_usage(
    State(state): State<AppState>,
) -> Result<Json<Vec<usage::DailyUsage>>, UsageApiError> {
    Ok(Json(usage::daily(&state.db).await?))
}

struct UsageApiError(usage::UsageError);

impl From<usage::UsageError> for UsageApiError {
    fn from(value: usage::UsageError) -> Self {
        Self(value)
    }
}

impl axum::response::IntoResponse for UsageApiError {
    fn into_response(self) -> axum::response::Response {
        let status = match self.0 {
            usage::UsageError::BudgetExceeded => StatusCode::PAYMENT_REQUIRED,
            usage::UsageError::InvalidConfiguration => StatusCode::INTERNAL_SERVER_ERROR,
            usage::UsageError::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        (
            status,
            Json(ApiErrorBody {
                error: self.0.to_string(),
            }),
        )
            .into_response()
    }
}

async fn list_notes(
    State(state): State<AppState>,
) -> Result<Json<Vec<notes::NoteSummary>>, NoteApiError> {
    Ok(Json(notes::list(&state.db).await?))
}

async fn get_note(
    State(state): State<AppState>,
    Path(note_id): Path<i64>,
) -> Result<Json<notes::NoteSummary>, NoteApiError> {
    Ok(Json(notes::get(&state.db, note_id).await?))
}

async fn create_note(
    State(state): State<AppState>,
    Json(request): Json<notes::CreateNote>,
) -> Result<(StatusCode, Json<notes::NoteSummary>), NoteApiError> {
    Ok((
        StatusCode::CREATED,
        Json(notes::create(&state.db, request).await?),
    ))
}

async fn delete_note(
    State(state): State<AppState>,
    Path(note_id): Path<i64>,
) -> Result<StatusCode, NoteApiError> {
    notes::delete(&state.db, note_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

struct NoteApiError(notes::NoteError);

impl From<notes::NoteError> for NoteApiError {
    fn from(value: notes::NoteError) -> Self {
        Self(value)
    }
}

impl axum::response::IntoResponse for NoteApiError {
    fn into_response(self) -> axum::response::Response {
        let status = match self.0 {
            notes::NoteError::NotFound => StatusCode::NOT_FOUND,
            notes::NoteError::Duplicate => StatusCode::CONFLICT,
            notes::NoteError::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            _ => StatusCode::BAD_REQUEST,
        };
        (
            status,
            Json(ApiErrorBody {
                error: self.0.to_string(),
            }),
        )
            .into_response()
    }
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
    #[serde(default)]
    routing_mode: RoutingMode,
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

type ApiEventStream =
    std::pin::Pin<Box<dyn Stream<Item = Result<Event, std::convert::Infallible>> + Send>>;

async fn chat(
    State(state): State<AppState>,
    Json(request): Json<ManualCompletionRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, RouterApiError> {
    usage::enforce_budget(&state.db, &state.costs)
        .await
        .map_err(UsageApiError)
        .map_err(RouterApiError::Usage)?;
    let started_at = Instant::now();
    let request = validate_request(request)?;
    let grounded = crate::chat::prepare(
        &state.db,
        &request.prompt,
        request.model,
        request.routing_mode,
        request.max_output_tokens,
    )
    .await
    .map_err(SearchApiError::from)
    .map_err(RouterApiError::Search)?;

    let events: ApiEventStream = if let Some(grounded) = grounded {
        let sources = grounded.sources;
        let source_count = sources.len();
        let mut upstream = state.router.stream(grounded.completion).await?;
        let usage_db = state.db.clone();
        let cost_config = state.costs.clone();
        Box::pin(async_stream::stream! {
            yield Ok(sse_json("sources", &sources));
            let mut answer = String::new();
            let mut selected_model = "unknown".to_owned();
            let mut routing_mode = RoutingMode::Manual;
            let mut routing_reason: Option<String> = None;
            let mut classifier_confidence = None;
            let mut token_usage = None;
            while let Some(result) = upstream.next().await {
                match result {
                    Ok(StreamEvent::Delta { text }) => {
                        answer.push_str(&text);
                        yield Ok(stream_event(StreamEvent::Delta { text }));
                    }
                    Ok(StreamEvent::Done) => {}
                    Ok(StreamEvent::Metadata { requested_model, selected_model: model, routing_mode: mode, route, routing_reason: reason, classifier_confidence: confidence }) => {
                        selected_model = model.clone(); routing_mode = mode; routing_reason = reason.clone(); classifier_confidence = confidence;
                        yield Ok(stream_event(StreamEvent::Metadata { requested_model, selected_model: model, routing_mode: mode, route, routing_reason: reason, classifier_confidence: confidence }));
                    }
                    Ok(StreamEvent::Usage { usage }) => {
                        token_usage = Some(usage.clone());
                        yield Ok(stream_event(StreamEvent::Usage { usage }));
                    }
                    Err(error) => {
                        let _ = usage::record(&usage_db, &cost_config, usage::RunRecord { routing_mode, selected_model: &selected_model, selection_reason: routing_reason.as_deref(), classifier_confidence, usage: token_usage.as_ref(), latency_ms: started_at.elapsed().as_millis() as u64, status: "failed" }).await;
                        yield Ok(sse_json("error", &ApiErrorBody { error: error.to_string() }));
                        return;
                    }
                }
            }
            let validation = crate::chat::validate_citations(&answer, source_count);
            yield Ok(sse_json("citation_validation", &validation));
            let _ = usage::record(&usage_db, &cost_config, usage::RunRecord { routing_mode, selected_model: &selected_model, selection_reason: routing_reason.as_deref(), classifier_confidence, usage: token_usage.as_ref(), latency_ms: started_at.elapsed().as_millis() as u64, status: "completed" }).await;
            yield Ok(stream_event(StreamEvent::Done));
        })
    } else {
        Box::pin(async_stream::stream! {
            yield Ok(sse_json("sources", &Vec::<crate::chat::GroundedSource>::new()));
            yield Ok(stream_event(StreamEvent::Delta {
                text: "Not found in the supplied sources. Try using specific rules or campaign terms, or search the archive directly.".into(),
            }));
            yield Ok(sse_json("citation_validation", &crate::chat::CitationValidation {
                cited: Vec::new(), unsupported: Vec::new(), missing_required: false,
            }));
            yield Ok(stream_event(StreamEvent::Done));
        })
    };

    Ok(Sse::new(events).keep_alive(KeepAlive::default()))
}

fn sse_json<T: Serialize>(name: &'static str, value: &T) -> Event {
    Event::default()
        .event(name)
        .json_data(value)
        .expect("API event should serialize")
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
        instructions: None,
        prompt: prompt.to_owned(),
        model: request.model,
        routing_mode: request.routing_mode,
        max_output_tokens,
    })
}

enum RouterApiError {
    InvalidPrompt(String),
    Router(RouterError),
    Search(SearchApiError),
    Usage(UsageApiError),
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
            Self::Search(error) => error.into_response(),
            Self::Usage(error) => error.into_response(),
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
        sqlx::migrate!("./migrations").run(&db).await.unwrap();
        AppState {
            db,
            router,
            costs: usage::CostConfig::default(),
        }
    }

    async fn searchable_test_state(router: SharedModelRouter) -> AppState {
        let state = test_state(router).await;
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
            requested_model: ModelTier::Mini,
            selected_model: "gpt-5-mini".into(),
            routing_mode: RoutingMode::Manual,
            routing_reason: None,
            classifier_confidence: None,
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
    async fn grounded_chat_retrieves_sources_before_calling_router() {
        let router = mock_router();
        let response = app(searchable_test_state(router.clone()).await)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/chat")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"prompt":"What does prone do?","model":"mini","routing_mode":"auto"}"#,
                    ))
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
        assert!(text.contains("event: sources"));
        assert!(text.contains("\"citation_id\":\"S1\""));
        assert!(text.contains("event: citation_validation"));

        let calls = router.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].routing_mode, RoutingMode::Auto);
        assert!(
            calls[0]
                .instructions
                .as_deref()
                .unwrap()
                .contains("Answer only from")
        );
        assert!(calls[0].prompt.contains("\"id\":\"S1\""));
        assert!(calls[0].prompt.contains("only movement option"));
    }

    #[tokio::test]
    async fn grounded_chat_skips_model_when_no_source_matches() {
        let router = mock_router();
        let response = app(searchable_test_state(router.clone()).await)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/chat")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"prompt":"zyxwvu","model":"gpt-5"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
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
        assert!(text.contains("Not found in the supplied sources"));
        assert!(router.calls().is_empty());
    }

    #[tokio::test]
    async fn campaign_note_api_indexes_lists_and_deletes_a_note() {
        let state = searchable_test_state(mock_router()).await;
        let service = app(state.clone());
        let response = service
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/notes")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r##"{"title":"Ashen Vale","filename":"ashen.md","content":"# Lore\n\nThe moon gate opens with a silver key."}"##,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let note_id: i64 = sqlx::query_scalar(
            "SELECT id FROM documents WHERE kind = 'campaign_note' AND title = 'Ashen Vale'",
        )
        .fetch_one(&state.db)
        .await
        .unwrap();

        let list_response = service
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/notes")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let list_text = String::from_utf8(
            list_response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap();
        assert!(list_text.contains("Ashen Vale"));

        let delete_response = service
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/notes/{note_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);
        assert!(notes::list(&state.db).await.unwrap().is_empty());
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
