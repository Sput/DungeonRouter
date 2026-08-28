# Architecture

DungeonRouter is a local-first web application with three runtime processes:

```text
Browser (React/TypeScript, :5173)
        │ /api, SSE
        ▼
Rust API (Axum, :4000) ─────► SQLite (SRD, notes, usage)
        │ OpenAI-compatible chat request
        ▼
Switchyard (:4100) ─────────► OpenAI Responses API
```

The browser never receives the OpenAI credential. It sends questions and model preferences to the Rust API, renders streamed server-sent events, and loads cited passages by local chunk ID.

The API validates input, searches SRD and campaign-note indexes separately, merges relevant passages, and serializes them as untrusted JSON Lines reference data. It then calls the internal `ModelRouter` interface. This boundary contains Switchyard-specific behavior and permits another router implementation later.

Manual requests use Switchyard passthrough routes. Auto requests use a Nano classifier with a strict three-target schema and Mini fallback. Switchyard translates the OpenAI-compatible request and streams the chosen model's response back to Rust.

SQLite stores the bundled SRD, private campaign-note chunks, FTS5 indexes, and usage metadata. Ordinary usage records do not retain questions, answers, or note contents. The API enforces the configured monthly hard limit before starting a model call.

## Trust boundaries

- SRD and campaign-note content is untrusted data, even when included in a model prompt.
- Campaign passages leave the machine only when retrieval selects them for a model request.
- Provider and router errors are normalized before reaching the browser.
- The app binds to loopback by default and has no authentication; it should not be exposed to a network.
- Cost estimates depend on returned token counts and configured prices, while provider billing remains authoritative.
