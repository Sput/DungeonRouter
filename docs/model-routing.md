# Model routing

DungeonRouter isolates model access behind the Rust `ModelRouter` trait. The initial adapter sends OpenAI-compatible Chat Completions requests to a separately running Switchyard server. Switchyard translates those requests to the OpenAI Responses API and keeps the OpenAI credential out of the browser and repository configuration.

## Routes

| API value | Switchyard route | OpenAI model |
|---|---|---|
| `nano` | `dungeon-router/nano` | `gpt-5-nano` |
| `mini` | `dungeon-router/mini` | `gpt-5-mini` |
| `gpt-5` | `dungeon-router/gpt-5` | `gpt-5` |
| Auto | `dungeon-router/auto` | selected by Switchyard |

Auto is the browser default. Switchyard's custom multi-target `llm_classifier` route uses `gpt-5-nano` as the classifier and selects the cheapest appropriate answer target:

- Nano for direct definitions and one-passage lookups;
- Mini for multi-rule explanations and ordinary adjudication;
- GPT-5 for materially ambiguous or deeply interacting rules.

The classifier returns a strict JSON-schema verdict containing target, task type, reason, and confidence. Switchyard validates the target through a deterministic JSON-pointer policy. Invalid output or classifier failure falls back to Mini; it never silently escalates to GPT-5. Choosing Nano, Mini, or GPT-5 manually uses the passthrough route and does not incur a classifier call.

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

The routing receipt uses Switchyard's `x-model-router-selected-model` and `x-model-router-rationale` response headers. It displays the actual selected model, route, reason, and classifier confidence when the rationale includes one.

The user-facing rules flow now uses `POST /api/chat`, which performs local SRD retrieval before calling this streaming router. See [Grounded chat](grounded-chat.md). The lower-level `/api/router/*` endpoints remain available for adapter diagnostics.

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
