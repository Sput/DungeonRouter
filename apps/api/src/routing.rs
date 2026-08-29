use std::{fmt, pin::Pin, sync::Arc, time::Duration};

use async_trait::async_trait;
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use futures_core::Stream;
use futures_util::StreamExt;

const NANO_ROUTE: &str = "dungeon-router/nano";
const MINI_ROUTE: &str = "dungeon-router/mini";
const GPT5_ROUTE: &str = "dungeon-router/gpt-5";
const AUTO_ROUTE: &str = "dungeon-router/auto";

pub type SharedModelRouter = Arc<dyn ModelRouter>;
pub type RouterStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, RouterError>> + Send>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelTier {
    Nano,
    Mini,
    #[serde(rename = "gpt-5")]
    Gpt5,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RoutingMode {
    Auto,
    #[default]
    Manual,
}

impl ModelTier {
    pub const ALL: [Self; 3] = [Self::Nano, Self::Mini, Self::Gpt5];

    pub const fn model_id(self) -> &'static str {
        match self {
            Self::Nano => "gpt-5-nano",
            Self::Mini => "gpt-5-mini",
            Self::Gpt5 => "gpt-5",
        }
    }

    pub const fn route_id(self) -> &'static str {
        match self {
            Self::Nano => NANO_ROUTE,
            Self::Mini => MINI_ROUTE,
            Self::Gpt5 => GPT5_ROUTE,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Nano => "Nano",
            Self::Mini => "Mini",
            Self::Gpt5 => "GPT-5",
        }
    }

    pub const fn purpose(self) -> &'static str {
        match self {
            Self::Nano => "Direct lookup and short grounded answers",
            Self::Mini => "Multi-rule explanation and ordinary adjudication",
            Self::Gpt5 => "Difficult reasoning and ambiguous interactions",
        }
    }
}

impl fmt::Display for ModelTier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelDescriptor {
    pub tier: ModelTier,
    pub label: &'static str,
    pub model_id: &'static str,
    pub purpose: &'static str,
}

pub fn model_catalog() -> Vec<ModelDescriptor> {
    ModelTier::ALL
        .into_iter()
        .map(|tier| ModelDescriptor {
            tier,
            label: tier.label(),
            model_id: tier.model_id(),
            purpose: tier.purpose(),
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub instructions: Option<String>,
    pub prompt: String,
    pub model: ModelTier,
    pub routing_mode: RoutingMode,
    pub max_output_tokens: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompletionResponse {
    pub content: String,
    pub requested_model: ModelTier,
    pub selected_model: String,
    pub routing_mode: RoutingMode,
    pub routing_reason: Option<String>,
    pub classifier_confidence: Option<f64>,
    pub usage: Option<TokenUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    Metadata {
        requested_model: ModelTier,
        selected_model: String,
        routing_mode: RoutingMode,
        route: String,
        routing_reason: Option<String>,
        classifier_confidence: Option<f64>,
    },
    Delta {
        text: String,
    },
    Usage {
        usage: TokenUsage,
    },
    Done,
}

#[derive(Debug, Error)]
pub enum RouterError {
    #[error("the model router is unavailable")]
    Unavailable,
    #[error("the model provider rejected the request")]
    Rejected,
    #[error("the model router returned an invalid response")]
    InvalidResponse,
}

impl IntoResponse for RouterError {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Rejected => StatusCode::BAD_GATEWAY,
            Self::Unavailable | Self::InvalidResponse => StatusCode::SERVICE_UNAVAILABLE,
        };
        (
            status,
            Json(ErrorBody {
                error: self.to_string(),
            }),
        )
            .into_response()
    }
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

#[async_trait]
pub trait ModelRouter: Send + Sync {
    async fn complete(&self, request: CompletionRequest)
    -> Result<CompletionResponse, RouterError>;

    async fn stream(&self, request: CompletionRequest) -> Result<RouterStream, RouterError>;
}

#[derive(Clone)]
pub struct SwitchyardRouter {
    client: Client,
    base_url: String,
}

impl SwitchyardRouter {
    pub fn new(base_url: impl Into<String>) -> Result<Self, reqwest::Error> {
        let client = Client::builder().timeout(Duration::from_secs(90)).build()?;
        Ok(Self::with_client(base_url, client))
    }

    pub fn with_client(base_url: impl Into<String>, client: Client) -> Self {
        Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_owned(),
        }
    }
}

#[async_trait]
impl ModelRouter for SwitchyardRouter {
    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, RouterError> {
        let endpoint = format!("{}/v1/chat/completions", self.base_url);
        let payload = SwitchyardRequest {
            model: request.route_id(),
            messages: request_messages(&request),
            stream: false,
            stream_options: None,
            max_completion_tokens: request.max_output_tokens,
        };

        let response = self
            .client
            .post(endpoint)
            .json(&payload)
            .send()
            .await
            .map_err(|_| RouterError::Unavailable)?;

        if !response.status().is_success() {
            tracing::warn!(status = %response.status(), "Switchyard rejected completion request");
            return Err(RouterError::Rejected);
        }

        let selected_model = selected_model_from_headers(response.headers());
        let routing_reason = routing_reason_from_headers(response.headers());
        let response: SwitchyardResponse = response
            .json()
            .await
            .map_err(|_| RouterError::InvalidResponse)?;
        let content = response
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message.content)
            .filter(|content| !content.trim().is_empty())
            .ok_or(RouterError::InvalidResponse)?;

        let selected_model = selected_model.unwrap_or(response.model);
        let (routing_reason, classifier_confidence) =
            routing_receipt(request.routing_mode, &selected_model, routing_reason);
        Ok(CompletionResponse {
            content,
            requested_model: request.model,
            selected_model,
            routing_mode: request.routing_mode,
            routing_reason,
            classifier_confidence,
            usage: response.usage.map(Into::into),
        })
    }

    async fn stream(&self, request: CompletionRequest) -> Result<RouterStream, RouterError> {
        let endpoint = format!("{}/v1/chat/completions", self.base_url);
        let payload = SwitchyardRequest {
            model: request.route_id(),
            messages: request_messages(&request),
            stream: true,
            stream_options: Some(StreamOptions {
                include_usage: true,
            }),
            max_completion_tokens: request.max_output_tokens,
        };

        let response = self
            .client
            .post(endpoint)
            .json(&payload)
            .send()
            .await
            .map_err(|_| RouterError::Unavailable)?;

        if !response.status().is_success() {
            tracing::warn!(status = %response.status(), "Switchyard rejected streaming request");
            return Err(RouterError::Rejected);
        }

        let requested_model = request.model;
        let selected_model = selected_model_from_headers(response.headers())
            .unwrap_or_else(|| requested_model.model_id().to_owned());
        let routing_reason = routing_reason_from_headers(response.headers());
        let routing_mode = request.routing_mode;
        let route = request.route_id().to_owned();
        let (routing_reason, classifier_confidence) =
            routing_receipt(routing_mode, &selected_model, routing_reason);
        let mut bytes = response.bytes_stream();

        let stream = async_stream::try_stream! {
            let mut produced_text = false;
            yield StreamEvent::Metadata {
                requested_model,
                selected_model,
                routing_mode,
                route,
                routing_reason,
                classifier_confidence,
            };

            let mut buffer = Vec::new();
            while let Some(chunk) = bytes.next().await {
                let chunk = chunk.map_err(|_| RouterError::Unavailable)?;
                buffer.extend_from_slice(&chunk);

                while let Some((boundary, delimiter_length)) = find_sse_boundary(&buffer) {
                    let frame = std::str::from_utf8(&buffer[..boundary])
                        .map_err(|_| RouterError::InvalidResponse)?
                        .to_owned();
                    buffer.drain(..boundary + delimiter_length);
                    if let Some(event) = parse_switchyard_frame(&frame)? {
                        if matches!(&event, StreamEvent::Delta { text } if !text.trim().is_empty()) {
                            produced_text = true;
                        }
                        yield event;
                    }
                }
            }

            let trailing = std::str::from_utf8(&buffer).map_err(|_| RouterError::InvalidResponse)?;
            let trailing_event = (!trailing.trim().is_empty())
                .then(|| parse_switchyard_frame(trailing))
                .transpose()?
                .flatten();
            if let Some(event) = trailing_event {
                if matches!(&event, StreamEvent::Delta { text } if !text.trim().is_empty()) {
                    produced_text = true;
                }
                yield event;
            }
            if !produced_text {
                Err(RouterError::InvalidResponse)?;
            }
            yield StreamEvent::Done;
        };

        Ok(Box::pin(stream))
    }
}

#[derive(Serialize)]
struct SwitchyardRequest<'a> {
    model: &'static str,
    messages: Vec<ChatMessage<'a>>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptions>,
    max_completion_tokens: u32,
}

#[derive(Serialize)]
struct StreamOptions {
    include_usage: bool,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'static str,
    content: &'a str,
}

fn request_messages(request: &CompletionRequest) -> Vec<ChatMessage<'_>> {
    let mut messages = Vec::with_capacity(2);
    if let Some(instructions) = request.instructions.as_deref() {
        messages.push(ChatMessage {
            role: "developer",
            content: instructions,
        });
    }
    messages.push(ChatMessage {
        role: "user",
        content: &request.prompt,
    });
    messages
}

#[derive(Deserialize)]
struct SwitchyardResponse {
    model: String,
    choices: Vec<Choice>,
    usage: Option<SwitchyardUsage>,
}

#[derive(Deserialize)]
struct Choice {
    message: AssistantMessage,
}

#[derive(Deserialize)]
struct AssistantMessage {
    content: String,
}

#[derive(Deserialize)]
struct SwitchyardUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
}

#[derive(Deserialize)]
struct SwitchyardStreamChunk {
    model: Option<String>,
    #[serde(default)]
    choices: Vec<StreamChoice>,
    usage: Option<SwitchyardUsage>,
}

#[derive(Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
}

#[derive(Deserialize)]
struct StreamDelta {
    content: Option<String>,
}

fn selected_model_from_headers(headers: &reqwest::header::HeaderMap) -> Option<String> {
    [
        "x-model-router-selected-model",
        "x-switchyard-selected-model",
    ]
    .into_iter()
    .find_map(|name| headers.get(name)?.to_str().ok().map(str::to_owned))
}

fn routing_reason_from_headers(headers: &reqwest::header::HeaderMap) -> Option<String> {
    headers
        .get("x-model-router-rationale")?
        .to_str()
        .ok()
        .map(str::to_owned)
}

fn parse_confidence(reason: &str) -> Option<f64> {
    let start = reason.find("confidence")? + "confidence".len();
    let value = reason[start..]
        .trim_start_matches(|character: char| {
            character.is_whitespace() || matches!(character, ':' | '=' | '(')
        })
        .split(|character: char| !(character.is_ascii_digit() || character == '.'))
        .next()?;
    value.parse().ok()
}

fn fallback_routing_reason(mode: RoutingMode, selected_model: &str) -> String {
    match mode {
        RoutingMode::Auto => {
            format!("Switchyard selected {selected_model}; classifier details were not returned")
        }
        RoutingMode::Manual => format!("The DM manually selected {selected_model}"),
    }
}

fn routing_receipt(
    mode: RoutingMode,
    selected_model: &str,
    upstream_reason: Option<String>,
) -> (Option<String>, Option<f64>) {
    let confidence = upstream_reason.as_deref().and_then(parse_confidence);
    if mode == RoutingMode::Auto
        && confidence == Some(0.0)
        && upstream_reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("fall-through selected"))
    {
        return (
            Some(format!(
                "Classifier unavailable; Switchyard used the {selected_model} safety fallback"
            )),
            None,
        );
    }
    (
        Some(upstream_reason.unwrap_or_else(|| fallback_routing_reason(mode, selected_model))),
        confidence,
    )
}

impl CompletionRequest {
    fn route_id(&self) -> &'static str {
        match self.routing_mode {
            RoutingMode::Auto => AUTO_ROUTE,
            RoutingMode::Manual => self.model.route_id(),
        }
    }
}

fn find_sse_boundary(buffer: &[u8]) -> Option<(usize, usize)> {
    let line_feed = buffer.windows(2).position(|window| window == b"\n\n");
    let carriage_return = buffer.windows(4).position(|window| window == b"\r\n\r\n");
    match (line_feed, carriage_return) {
        (Some(lf), Some(crlf)) if lf < crlf => Some((lf, 2)),
        (Some(_), Some(crlf)) => Some((crlf, 4)),
        (Some(lf), None) => Some((lf, 2)),
        (None, Some(crlf)) => Some((crlf, 4)),
        (None, None) => None,
    }
}

fn parse_switchyard_frame(frame: &str) -> Result<Option<StreamEvent>, RouterError> {
    let data = frame
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n");
    if data.is_empty() || data == "[DONE]" {
        return Ok(None);
    }

    let chunk: SwitchyardStreamChunk =
        serde_json::from_str(&data).map_err(|_| RouterError::InvalidResponse)?;
    if let Some(usage) = chunk.usage {
        return Ok(Some(StreamEvent::Usage {
            usage: usage.into(),
        }));
    }
    let _reported_model = chunk.model;
    Ok(chunk
        .choices
        .into_iter()
        .find_map(|choice| choice.delta.content)
        .filter(|content| !content.is_empty())
        .map(|text| StreamEvent::Delta { text }))
}

impl From<SwitchyardUsage> for TokenUsage {
    fn from(value: SwitchyardUsage) -> Self {
        Self {
            input_tokens: value.prompt_tokens,
            output_tokens: value.completion_tokens,
            total_tokens: value.total_tokens,
        }
    }
}

#[cfg(test)]
pub mod testing {
    use std::sync::Mutex;

    use super::*;

    pub struct MockRouter {
        calls: Mutex<Vec<CompletionRequest>>,
        response: CompletionResponse,
    }

    impl MockRouter {
        pub fn new(response: CompletionResponse) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                response,
            }
        }

        pub fn calls(&self) -> Vec<CompletionRequest> {
            self.calls.lock().expect("calls lock poisoned").clone()
        }
    }

    #[async_trait]
    impl ModelRouter for MockRouter {
        async fn complete(
            &self,
            request: CompletionRequest,
        ) -> Result<CompletionResponse, RouterError> {
            self.calls
                .lock()
                .expect("calls lock poisoned")
                .push(request);
            Ok(self.response.clone())
        }

        async fn stream(&self, request: CompletionRequest) -> Result<RouterStream, RouterError> {
            let requested_model = request.model;
            let routing_mode = request.routing_mode;
            self.calls
                .lock()
                .expect("calls lock poisoned")
                .push(request);
            let mut events = vec![
                Ok(StreamEvent::Metadata {
                    requested_model,
                    selected_model: self.response.selected_model.clone(),
                    routing_mode,
                    route: match routing_mode {
                        RoutingMode::Auto => AUTO_ROUTE,
                        RoutingMode::Manual => requested_model.route_id(),
                    }
                    .to_owned(),
                    routing_reason: self.response.routing_reason.clone(),
                    classifier_confidence: self.response.classifier_confidence,
                }),
                Ok(StreamEvent::Delta {
                    text: self.response.content.clone(),
                }),
            ];
            if let Some(usage) = self.response.usage.clone() {
                events.push(Ok(StreamEvent::Usage { usage }));
            }
            events.push(Ok(StreamEvent::Done));
            Ok(Box::pin(futures_util::stream::iter(events)))
        }
    }
}

#[cfg(test)]
mod adapter_tests {
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_partial_json, method, path},
    };

    use super::*;

    #[test]
    fn auto_requests_use_classifier_route_and_parse_receipt_confidence() {
        let request = CompletionRequest {
            instructions: None,
            prompt: "question".into(),
            model: ModelTier::Mini,
            routing_mode: RoutingMode::Auto,
            max_output_tokens: 100,
        };
        assert_eq!(request.route_id(), "dungeon-router/auto");
        assert_eq!(
            parse_confidence("llm_classifier selected nano (confidence 0.91)"),
            Some(0.91)
        );
        let (reason, confidence) = routing_receipt(
            RoutingMode::Auto,
            "gpt-5-mini",
            Some("fall-through selected gpt-5-mini (confidence 0.000)".into()),
        );
        assert_eq!(confidence, None);
        assert!(reason.unwrap().contains("safety fallback"));
    }

    #[tokio::test]
    async fn switchyard_adapter_uses_the_manual_route_and_maps_usage() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_partial_json(json!({
                "model": "dungeon-router/gpt-5",
                "messages": [{"role": "user", "content": "Resolve this interaction"}],
                "stream": false,
                "max_completion_tokens": 700
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "model": "gpt-5-2025-08-07",
                "choices": [{"message": {"content": "A defensible ruling"}}],
                "usage": {
                    "prompt_tokens": 42,
                    "completion_tokens": 18,
                    "total_tokens": 60
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let router = SwitchyardRouter::new(server.uri()).expect("HTTP client should build");
        let response = router
            .complete(CompletionRequest {
                instructions: None,
                prompt: "Resolve this interaction".into(),
                model: ModelTier::Gpt5,
                routing_mode: RoutingMode::Manual,
                max_output_tokens: 700,
            })
            .await
            .expect("mock completion should succeed");

        assert_eq!(response.requested_model, ModelTier::Gpt5);
        assert_eq!(response.selected_model, "gpt-5-2025-08-07");
        assert_eq!(response.content, "A defensible ruling");
        let usage = response.usage.expect("usage should be mapped");
        assert_eq!(usage.input_tokens, 42);
        assert_eq!(usage.output_tokens, 18);
        assert_eq!(usage.total_tokens, 60);
    }

    #[tokio::test]
    async fn switchyard_adapter_does_not_leak_upstream_error_bodies() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(401)
                    .set_body_string("secret upstream diagnostic must not escape"),
            )
            .mount(&server)
            .await;

        let router = SwitchyardRouter::new(server.uri()).expect("HTTP client should build");
        let error = router
            .complete(CompletionRequest {
                instructions: None,
                prompt: "question".into(),
                model: ModelTier::Nano,
                routing_mode: RoutingMode::Manual,
                max_output_tokens: 100,
            })
            .await
            .expect_err("upstream rejection should fail");

        assert_eq!(error.to_string(), "the model provider rejected the request");
    }

    #[tokio::test]
    async fn switchyard_adapter_maps_streaming_frames_and_router_header() {
        let server = MockServer::start().await;
        let body = concat!(
            "data: {\"model\":\"gpt-5-mini\",\"choices\":[{\"delta\":{\"content\":\"Prone \"}}]}\n\n",
            "data: {\"model\":\"gpt-5-mini\",\"choices\":[{\"delta\":{\"content\":\"limits movement.\"}}]}\n\n",
            "data: {\"model\":\"gpt-5-mini\",\"choices\":[],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":4,\"total_tokens\":16}}\n\n",
            "data: [DONE]\n\n"
        );
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_partial_json(json!({
                "model": "dungeon-router/mini",
                "stream": true,
                "stream_options": {"include_usage": true}
            })))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .insert_header("x-model-router-selected-model", "gpt-5-mini")
                    .set_body_string(body),
            )
            .mount(&server)
            .await;

        let router = SwitchyardRouter::new(server.uri()).expect("HTTP client should build");
        let events = router
            .stream(CompletionRequest {
                instructions: None,
                prompt: "What does prone do?".into(),
                model: ModelTier::Mini,
                routing_mode: RoutingMode::Manual,
                max_output_tokens: 300,
            })
            .await
            .expect("stream should start")
            .collect::<Vec<_>>()
            .await;

        assert!(matches!(
            &events[0],
            Ok(StreamEvent::Metadata { selected_model, .. }) if selected_model == "gpt-5-mini"
        ));
        assert!(matches!(&events[1], Ok(StreamEvent::Delta { text }) if text == "Prone "));
        assert!(
            matches!(&events[2], Ok(StreamEvent::Delta { text }) if text == "limits movement.")
        );
        assert!(matches!(
            &events[3],
            Ok(StreamEvent::Usage { usage }) if usage.total_tokens == 16
        ));
        assert!(matches!(&events[4], Ok(StreamEvent::Done)));
    }
}
