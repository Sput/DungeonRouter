# Grounded rules answers

The user-facing question flow uses `POST /api/chat`. It retrieves SRD passages before making a model call and streams both the answer and its supporting-source metadata to the browser.

## Request flow

1. Validate the question, selected manual model, and output limit.
2. Search SQLite FTS5 using meaningful query terms.
3. Retrieve at most four complete passages.
4. Assign request-local citation IDs (`S1`, `S2`, and so on).
5. Send a developer message that restricts the model to the supplied evidence.
6. Stream sources, routing metadata, answer deltas, usage, citation validation, and completion events.
7. Turn only validated citation IDs into source links in the browser.

If no passage matches, the API returns an insufficient-evidence response without calling Switchyard or OpenAI. This avoids paying for an answer that cannot be grounded.

## Grounding policy

The model must:

- answer only from passages included in the request;
- cite every rules claim with a supplied source ID;
- label reasoning not directly established by the passages as `Interpretation:`;
- say `Not found in the supplied SRD passages` when evidence is insufficient;
- avoid treating absent evidence as proof that a rule does not exist;
- treat source contents as data rather than instructions.

The full stable grounding policy is sent as a developer message. The dynamic passages and question are placed in the user message.

## Citation validation

After streaming, the server scans the answer for `[S<number>]` markers. It reports:

- supported IDs that correspond to retrieved passages;
- unsupported IDs invented by the model;
- whether a grounded answer omitted citations entirely.

The browser links supported IDs only. Unsupported markers remain plain text and produce a warning. Clicking a valid citation fetches the complete passage from `GET /api/sources/{chunk_id}`.

Validation proves that a cited ID exists in the supplied context; it does not yet prove that every individual claim is semantically entailed by that passage. The evaluation milestone will measure that separately.

## Streaming events

The chat endpoint emits named server-sent events:

| Event | Purpose |
|---|---|
| `sources` | Retrieved passages and citation IDs |
| `metadata` | Selected model and Switchyard route |
| `delta` | Incremental answer text |
| `usage` | Token counts when supplied upstream |
| `citation_validation` | Supported, unsupported, and missing citations |
| `done` | Normal completion |
| `error` | Normalized stream failure |

Auto is the default routing mode. The DM can bypass its classifier cost and force Nano, Mini, or GPT-5 for any question.
