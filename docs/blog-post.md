# Building DungeonRouter: Cost-Aware LLM Routing for D&D Rules

Large language models are good at answering tabletop role-playing questions, but using the largest model for every question is wasteful. “What does prone do?” does not require the same reasoning capacity as a dispute involving readied actions, reactions, concentration, and several plausible interpretations of timing.

DungeonRouter is an experimental, open-source D&D 5e rules assistant built around that observation. It searches the 2014 SRD 5.1 and private campaign notes, cites retrieved passages, and routes each question among `gpt-5-nano`, `gpt-5-mini`, and `gpt-5`. NVIDIA NeMo Switchyard performs the model selection, while a Rust API owns retrieval, policy enforcement, streaming, and usage accounting.

The goal was not simply to put a chat interface in front of an LLM. The interesting engineering problem was building a system that could answer quickly during a game, show its evidence, expose its routing decision, and stay within a small monthly budget.

## The architecture

DungeonRouter runs as three local processes:

```text
React/TypeScript browser (:5173)
          │ HTTP + Server-Sent Events
          ▼
Rust/Axum API (:4000) ─────► SQLite + FTS5
          │ OpenAI-compatible request
          ▼
NVIDIA NeMo Switchyard (:4100)
          │
          ▼
OpenAI API
```

The browser never receives the OpenAI API key. It sends a question and routing preference to the Rust service, then renders sources, answer fragments, model metadata, token usage, and validation results as Server-Sent Events.

The backend is responsible for everything that should remain deterministic: input validation, retrieval, citation identifiers, budget enforcement, error normalization, and usage records. Model access sits behind a Rust `ModelRouter` trait, so Switchyard is an adapter rather than a dependency spread throughout the application.

This boundary matters. Retrieval and citation behavior should not change merely because the model router is replaced later.

## Retrieval before generation

SRD 5.1 is normalized into bounded Markdown chunks and bundled as a search-ready snapshot. On first startup, the API indexes those chunks in SQLite. Campaign notes supplied as Markdown or plain text are stored and indexed separately.

For each question, the API:

1. Searches both indexes with SQLite FTS5.
2. Merges the results by relevance.
3. Loads no more than four complete passages.
4. Assigns request-local identifiers such as `S1` and `S2`.
5. Serializes the passages as JSON Lines and sends them as untrusted reference data.

FTS5 was a deliberate MVP choice. It is local, inexpensive, deterministic, and good at matching rules terminology. It does not handle semantic paraphrases as well as vector retrieval, but adding embeddings before measuring actual retrieval failures would have increased cost and operational complexity prematurely.

The first grounding policy was intentionally strict: answer only from retrieved passages. That worked well for direct lookups but failed on a useful real-world question:

> What is a good spell for a divination wizard to use against a red dragon in a confined space?

Retrieval found red-dragon statistics but not enough material about the wizard subclass or suitable spells. The model correctly refused to invent a sourced answer, but the result was not useful to a DM.

The revised policy separates provenance instead of treating grounding as all-or-nothing. Claims derived from retrieved passages receive citations. Additional guidance may use the selected model’s general 2014 D&D knowledge, but it must appear under a visible `Model knowledge (not source-verified):` heading. The UI also displays a warning for that portion.

This preserves the distinction between evidence and model memory without reducing the application to a search engine that refuses every question outside its corpus.

## Routing by required reasoning

Auto mode uses a Nano classifier and a strict JSON Schema verdict. The classifier selects one of three Switchyard targets:

| Tier | Intended workload |
|---|---|
| GPT-5 Nano | Definitions, extraction, and direct one-passage lookups |
| GPT-5 Mini | Multi-rule explanations, routine adjudication, and tactical recommendations |
| GPT-5 | Ambiguous timing, competing interpretations, and long chains of interacting effects |

The important phrase is “required reasoning.” Routing on prompt length or keywords alone would be fragile. A long question can still be a simple lookup, while a short question can hide a difficult timing problem.

The classifier applies its rules in priority order. GPT-5 has hard escalation triggers such as reactions interrupting other actions, three or more interacting effects, conflicting rules, or an explicit request for every defensible ruling. Mini handles ordinary synthesis and recommendations. Nano is chosen only after the higher-complexity conditions have been ruled out.

Manual selection bypasses classification entirely. That makes it useful both for users who want control and for debugging the individual targets.

Switchyard returns the selected model and rationale in response headers. DungeonRouter turns that receipt into user-facing metadata, including the route and classifier confidence. If classification fails, Switchyard falls back to Mini rather than silently choosing the most expensive model.

## Streaming without hiding failures

The browser uses `fetch` rather than `EventSource` because the request is a POST containing the question. The Rust API forwards the streaming response and emits named events:

```text
sources
metadata
delta
usage
citation_validation
done
error
```

An `AbortController` implements Stop while preserving the partial answer. When the browser drops the body, Rust drops the upstream stream as well.

One subtle failure appeared during testing: the provider could return a nominally completed response containing reasoning tokens but no visible answer. Treating that as success produced an empty result in the UI. DungeonRouter now tracks whether any non-whitespace delta was produced and rejects empty streams explicitly.

This surfaced a related detail of reasoning models: `max_output_tokens` covers both internal reasoning and visible output. A model can exhaust the allowance before emitting user-facing text. Answer requests therefore have an 8,000-token ceiling, and the GPT-5 target uses low reasoning effort so complex rulings retain space for an answer. The ceiling is headroom, not a request to generate 8,000 tokens.

## Debugging the classifier boundary

Integrating a router revealed failures that would have been easy to misdiagnose as poor model judgment.

The first classifier failure was an HTTP 400 from OpenAI: `text.format.name` was missing. Switchyard 0.2.0 constructed a valid Chat-style JSON Schema wrapper internally but did not flatten that wrapper correctly for the Responses API. The practical workaround was to give the classifier a dedicated OpenAI Chat Completions client while leaving answer generation on the Responses API.

The next failure was different: the classifier request succeeded, but Nano returned no JSON before consuming its output allowance. Setting the classifier to minimal reasoning solved that problem and reduced its latency and cost.

Finally, a valid classifier repeatedly selected Mini for a deliberately difficult Counterspell and readied-action question. That was not an infrastructure failure; it was a policy failure. Rewriting the prompt with ordered rules, hard escalation triggers, and representative examples produced the intended GPT-5 route.

These cases reinforced an important observability lesson: “fallback selected Mini” is not enough diagnostic information. The system needs to distinguish an unavailable classifier, an invalid verdict, and a successful but undesirable classification.

## Citations and trust boundaries

The server validates every citation marker after generation. A marker is linkable only if it corresponds to a passage included in that request. Invented IDs remain plain text and generate a warning. Clicking a supported citation loads the complete local passage by chunk ID.

This validation proves that a cited source exists in the supplied context. It does not prove that the source semantically entails every nearby claim. Full entailment checking is a separate evaluation problem, but identifier validation still prevents a common and misleading failure: citations that look authoritative but point nowhere.

Campaign notes introduce another trust boundary. They are treated as untrusted data, not instructions. They remain in local SQLite storage and are sent to OpenAI only when retrieval selects them for a particular question. Ordinary usage records do not retain questions, answers, or note contents.

The application binds to loopback and has no authentication, so it is a local demonstration rather than a service that should be exposed directly to a network.

## Cost is a product feature

Cost awareness is visible rather than implicit. Each completed run records the selected model, routing mode, rationale, confidence, token counts, latency, estimated cost, and a comparison against always using GPT-5.

The default monthly configuration warns at $15 and blocks new model calls at $20. The hard limit is checked before retrieved context is sent upstream. Pricing is centralized and configurable because provider prices change; the provider invoice remains authoritative.

The classifier introduces a second model call in Auto mode, so routing is not free. For direct questions, the classifier and answer may both use Nano. That increases latency compared with manual Nano, but it allows the same interface to reserve GPT-5 for genuinely difficult requests. The right comparison is therefore not “one call versus two calls,” but the total cost and quality of the routed workload versus sending everything to the largest model.

## What the experiment demonstrated

DungeonRouter’s most useful result is not that three models can sit behind one dropdown. It is that model routing works best when it is part of a larger, observable decision system:

- Retrieval reduces the amount of knowledge the model must reconstruct.
- Citation validation gives users inspectable evidence.
- Explicit provenance keeps model knowledge separate from source-backed claims.
- Structured classification makes routing decisions machine-checkable.
- Manual routes make each tier independently testable.
- Safe fallback behavior contains failures without escalating cost.
- Token and latency receipts make optimization measurable.

There is still room to improve. FTS5 can be augmented with vector retrieval if evaluations show persistent semantic misses. Citation entailment can become stricter. Routing should be evaluated over a larger labeled question set, and latency could be reduced through caching or a cheaper non-LLM classifier for obvious lookups.

For a one-week experiment, however, the architecture demonstrates the core idea: a useful AI application does not need to choose between cheap models and capable models. It can make that choice per request—provided the routing policy, evidence, fallbacks, and costs are visible enough to debug.

DungeonRouter is available at [github.com/Sput/DungeonRouter](https://github.com/Sput/DungeonRouter).
