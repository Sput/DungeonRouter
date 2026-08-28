# Evaluation set

`questions.jsonl` contains 30 representative MVP questions covering direct lookup, multi-rule synthesis, complex adjudication, missing SRD material, campaign notes, and adversarial grounding attempts.

Each JSON object provides an ID, category, question, expected route, and expected source type. `expected_route: "none"` means retrieval should stop before a model call because the indexed sources do not support an answer.

For a live evaluation, seed campaign-note fixtures for the `campaign_note` and `mixed` cases, start Switchyard and the API with an OpenAI key, and submit each question through Auto mode. Record retrieval relevance, actual route, citation validity, latency, token counts, actual estimated cost, and the always-GPT-5 estimate. Live evaluation is intentionally not part of the normal test suite because it incurs API cost.

The repository tests validate that the dataset remains parseable and contains the required category coverage. Route quality and answer grounding still require a live scored run.
