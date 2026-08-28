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

The endpoint currently exists to validate routing independently of retrieval and chat streaming. The user-facing chat flow will use this same abstraction in the next milestone.

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
- verification that the selected model route and token usage are mapped correctly;
- verification that upstream error bodies are not exposed.

