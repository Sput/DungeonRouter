# Cost controls and usage

DungeonRouter records completed and failed model runs without storing the user's question or retrieved note text. Completed runs retain routing mode, selected model, routing rationale, classifier confidence, token counts, latency, estimated routed cost, and an always-GPT-5 comparison.

## Pricing

The default standard text-token prices are stored centrally and identified by pricing version `openai-2026-08-28`. Prices are USD per one million tokens:

| Model | Input | Output |
|---|---:|---:|
| GPT-5 Nano | $0.05 | $0.40 |
| GPT-5 Mini | $0.25 | $2.00 |
| GPT-5 | $1.25 | $10.00 |

Override any price with the corresponding variables in `.env.example`. Estimates use token counts returned by the answer-model stream. Switchyard does not currently expose the classifier's token usage through DungeonRouter's stream, so Auto estimates omit that small classifier cost. Provider invoices remain authoritative.

## Monthly budget

The defaults warn at $15 and stop new model calls at $20 during the current calendar month. `MONTHLY_COST_WARNING_USD` and `MONTHLY_COST_HARD_LIMIT_USD` configure those thresholds. The hard stop is enforced before retrieval context is sent to Switchyard. Raising it requires an explicit local configuration change and restart.

## API and dashboard

- `GET /api/config/public` returns public thresholds and the pricing version.
- `GET /api/activity?limit=20` returns recent model runs.
- `GET /api/usage/summary` returns monthly totals, model distribution, Auto/manual counts, an always-GPT-5 baseline, savings, and budget state.
- `GET /api/usage/daily` returns daily request and cost totals for up to 31 days.

The web dashboard refreshes after each completed answer and shows spend against the hard limit. Cost figures are estimates rather than billing guarantees.
