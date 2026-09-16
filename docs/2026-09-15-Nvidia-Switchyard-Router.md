# Building DungeonRouter: Cost-Aware LLM Routing for D&D Rules

Large language models are good at answering ambigious Dungeons and Dragons rules questions, but using the largest model for every question is wasteful. “What does prone do?” does not require the same reasoning capacity as a dispute involving readied actions, reactions, concentration, and several plausible interpretations of timing.

DungeonRouter is a D&D 5e rules assistant that searches the System Reference Document (SRD 5.1) and private campaign notes, cites retrieved passages, and routes each question among `gpt-5-nano`, `gpt-5-mini`, and `gpt-5`. NVIDIA NeMo Switchyard performs the model selection, while a Rust API owns retrieval, policy enforcement, streaming, and usage accounting.

Answering questions was solved by LLM Chatbots a few years ago, the goal here was to that to the next step and see how cheaply we can do it. 

## The architecture

DungeonRouter runs as three local processes:

![DungeonRouter architecture showing the browser, Rust API, local search, Switchyard, and OpenAI flow](assets/dungeonrouter-architecture.svg)

The browser sends a question and routing preference to the Rust service, then renders sources, answer fragments, model metadata, token usage, and validation results as Server-Sent Events.

The backend is responsible for everything that should remain deterministic: input validation, retrieval, citation identifiers, budget enforcement, error normalization, and usage records. 

## Retrieval before generation

SRD 5.1 is normalized into Markdown chunks and bundled as a search-ready snapshot. On first startup, the API indexes those chunks in SQLite. Campaign notes supplied as Markdown or plain text are stored and indexed separately.

For each question, the API:

1. Searches both indexes with SQLite FTS5.
2. Merges the results by relevance.
3. Loads no more than four complete passages.
4. Assigns request-local identifiers such as `S1` and `S2`.
5. Serializes the passages as JSON Lines and sends them as untrusted reference data.


## Routing by required reasoning

Routing starts by querying gpt-5-nano with the user's question and asking it to catgorize it using the following rules:

![Table of model tiers and intended workload: GPT-5 Nano handles definitions, extraction, and direct one-passage lookups; GPT-5 Mini handles multi-rule explanations, routine adjudication, and tactical recommendations; GPT-5 handles ambiguous timing, competing interpretations, and long chains of interacting effects](assets/dungeonrouter-model-tiers.svg)


The classifier applies its rules in priority order. GPT-5 has hard escalation triggers such as reactions interrupting other actions, three or more interacting effects, conflicting rules, or an explicit request for every defensible ruling. Mini handles ordinary synthesis and recommendations. Nano is chosen only after the higher-complexity conditions have been ruled out.

Manual selection bypasses classification entirely. That makes it useful both for users who want control and for debugging the individual targets.

Switchyard returns the selected model and rationale in response headers. DungeonRouter turns that receipt into user-facing metadata, including the route and classifier confidence. If classification fails, Switchyard falls back to Mini rather than silently choosing the most expensive model.

### Routing in practice

Two questions submitted to the same "Auto · cost-aware" endpoint land on different models, because the classifier reads how much reasoning each one actually needs.

![Query box asking "how much damage does a shortsword do?" with Attunement set to Auto, cost-aware](assets/dungeonrouter-example-nano-query.png)

![Result panel showing gpt-5-nano was selected via dungeon-router/auto with 100% confidence](assets/dungeonrouter-example-nano-result.png)

A direct SRD lookup like this falls through to `gpt-5-nano` with full confidence. There's no ambiguity to resolve and no synthesis across rules, so the cheapest capable model answers it.

![Query box asking for good wizard spells against a red dragon in a confined space, with Attunement set to Auto, cost-aware](assets/dungeonrouter-example-mini-query.png)

![Result panel showing gpt-5-mini was selected via dungeon-router/auto, using 4580 tokens over 59.8 seconds](assets/dungeonrouter-example-mini-result.png)

A tactical recommendation like this one has to weigh multiple interacting factors — area-of-effect damage, confined terrain, ally positioning — so it escalates to `gpt-5-mini`, which took 4,580 tokens and about a minute to produce a full answer. Same routing endpoint, same confidence in the decision, very different cost.

## Citations and trust boundaries

For the benefit of both the user and the application, everything that is used to answer the question is provided as a source. This allows the user to dig into the rules used to generate the answer if they wish.

Campaign notes introduce another trust boundary. They are treated as untrusted data, not instructions. They remain in local SQLite storage and are sent to OpenAI only when retrieval selects them for a particular question. 

## Cost is a product feature

Cost awareness is visible rather than implicit. Each completed run records the selected model, routing mode, rationale, confidence, token counts, latency, estimated cost, and a comparison against always using GPT-5.

## What the experiment demonstrated

This is a simple app that demonstrates how easy routing requests can be in order to save money on LLM queries. 

- Retrieval reduces the amount of knowledge being sent to the LLM, and therefore a further reduction in cost.
- Citation validation gives users inspectable evidence.
- Explicit provenance keeps model knowledge separate from source (both user and SRD).
- Structured classification makes routing decisions machine-checkable.
- Token and latency receipts make optimization measurable.