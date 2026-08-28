# Model routing

DungeonRouter isolates model access behind the Rust `ModelRouter` trait. The initial adapter sends OpenAI-compatible Chat Completions requests to a separately running Switchyard server. Switchyard translates those requests to the OpenAI Responses API and keeps the OpenAI credential out of the browser and repository configuration.

## Manual routes

| API value | Switchyard route | OpenAI model |
|---|---|---|
| `nano` | `dungeon-router/nano` | `gpt-5-nano` |
| `mini` | `dungeon-router/mini` | `gpt-5-mini` |
| `gpt-5` | `dungeon-router/gpt-5` | `gpt-5` |

Automatic classification is intentionally deferred until the automatic-routing milestone. Manual selection does not incur a classifier call.

## Start Switchyard

Install the standalone server using the current Switchyard installation instructions, then validate this repository's configuration:

```sh
switchyard-server --config config/switchyard.toml --dry-run
```

Start it on the port expected by `.env.example`:

```sh
switchyard-server \
  --config config/switchyard.toml \
  --host 127.0.0.1 \
  --port 4100
```

`OPENAI_API_KEY` must be present in the Switchyard process environment. The key is not placed in the TOML file, API responses, or frontend bundle.

## Backend endpoints

List supported manual models:

```text
GET /api/models
```

Run a non-streaming manual completion:

```text
POST /api/router/complete
Content-Type: application/json

{
  "prompt": "What does the prone condition do?",
  "model": "nano",
  "max_output_tokens": 500
}
```

Run a streaming manual completion:

```text
POST /api/router/stream
Accept: text/event-stream
Content-Type: application/json

{
  "prompt": "What does the prone condition do?",
  "model": "nano",
  "max_output_tokens": 800
}
```

The streaming endpoint returns named Server-Sent Events:

- `metadata` identifies the requested tier, selected upstream model, and Switchyard route;
- `delta` contains the next text fragment;
- `usage` reports token counts when the upstream provider supplies them;
- `done` marks normal completion;
- `error` reports a normalized mid-stream failure without exposing the provider body.

The React client reads these events using `fetch` so it can POST the question and cancel the request with an `AbortController`. Pressing Stop preserves the partial answer. When the browser drops the response body, Rust drops the upstream `reqwest` stream as well.

Automatic model selection is still intentionally deferred. Until that milestone, the UI defaults to the cheapest tier (`gpt-5-nano`) and lets the DM explicitly select Mini or GPT-5.

## Failure behavior

- Empty, oversized, or invalid requests receive HTTP 400.
- A rejected upstream request is normalized to HTTP 502.
- Transport failures and malformed upstream responses are normalized to HTTP 503.
- Raw provider error bodies are logged only as a status code and are never returned to the browser.
- The API does not automatically retry a request on a more expensive model.

## Testing

The test suite includes:

- handler tests using an in-memory `MockRouter`;
- validation tests ensuring bad requests do not call the router;
- HTTP adapter tests using a local mock server;
- streaming adapter and SSE handler tests using deterministic local responses;
- verification that the selected model route and token usage are mapped correctly;
- verification that upstream error bodies are not exposed.
