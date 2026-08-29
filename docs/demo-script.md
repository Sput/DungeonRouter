# Demonstration script

Target length: five to seven minutes.

## Before the demo

1. Copy `.env.example` to `.env`, add `OPENAI_API_KEY`, and leave the $20 hard limit in place.
2. Run `switchyard-server --config config/switchyard.toml --dry-run`.
3. Start Switchyard on `127.0.0.1:4100`, the Rust API on `127.0.0.1:4000`, and Vite on `127.0.0.1:5173`.
4. Confirm `curl http://127.0.0.1:4100/health` and `curl http://127.0.0.1:4000/api/health` succeed.
5. Upload a non-sensitive note containing: `# Ashen Vale\n\nThe moon gate opens with a silver key.`

## Walkthrough

1. Point out Auto mode, the local SQLite status, and the monthly cost gauge.
2. Ask: **What does the prone condition do?** Open its SRD citation and show the model receipt.
3. Ask: **Can a grappled creature stand up from prone?** Explain why synthesis should normally select Mini.
4. Ask: **Give two defensible rulings for simultaneous effects that reduce a creature to zero hit points.** Show the complex-routing result and its rationale.
5. Switch to a manual model and repeat one question to demonstrate classifier bypass.
6. Ask: **What opens the moon gate in the Ashen Vale?** Open the citation and point out its Campaign label.
7. Show the usage dashboard: routed spend, always-GPT-5 estimate, model distribution, latency, and $20 stop.
8. Close with the `ModelRouter` boundary: Switchyard is replaceable, while retrieval and UI remain unchanged.

## Recovery notes

- If Auto routing is surprising, explain that the routing receipt makes the decision inspectable and use a manual tier for the next request.
- If retrieval finds no evidence, show the model-knowledge disclosure and explain that those claims are useful guidance rather than source-verified rules.
- If Switchyard is unavailable, show SRD search, note management, the evaluation dataset, and automated test results without making a paid call.
- Never paste or display the API key during the demo.
