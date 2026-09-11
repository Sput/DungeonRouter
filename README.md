# DungeonRouter

DungeonRouter is an experimental, open-source D&D 5e (2014) rules assistant. It is designed to search SRD 5.1 and campaign notes, cite its sources, and demonstrate cost-aware routing across `gpt-5-nano`, `gpt-5-mini`, and `gpt-5` through NVIDIA NeMo Switchyard.

The repository currently contains the application foundation, manual model routing, and an end-to-end streaming chat path:

- a Rust/Axum API;
- SQLite migrations and startup initialization;
- a React/TypeScript/Vite frontend;
- a replaceable Rust `ModelRouter` abstraction;
- Switchyard passthrough routes for nano, mini, and GPT-5;
- streamed browser responses with stop-generation support and routing metadata;
- a bundled, CC-licensed SRD 5.1 snapshot with SQLite FTS5 retrieval;
- grounded streamed answers with validated, inspectable SRD citations;
- automatic cost-aware routing across Nano, Mini, and GPT-5;
- local Markdown and text campaign-note indexing with labeled citations;
- monthly usage accounting, configurable pricing, savings estimates, and a $20 hard stop;
- shared local-development commands and environment configuration.

## Repository layout

```text
apps/
  api/       Rust/Axum backend
  web/       React/TypeScript frontend
docs/        Architecture and project planning
```

## Prerequisites

- Rust and Cargo
- Node.js 20 or newer
- pnpm 9 or newer
- an OpenAI API key for live answers
- a Supabase project and anon key for authentication
- `switchyard-server` for live routing

An OpenAI API key is not required for unit tests or frontend development. It is required only when exercising live model calls through Switchyard.

## Local development

Copy the environment template:

```sh
cp .env.example .env
```

Add the Supabase project URL and anon key to `.env`, and add the corresponding `VITE_` values to `apps/web/.env.local`. See [Authentication](docs/authentication.md). Do not use a Supabase service-role or secret key.

Install frontend dependencies:

```sh
pnpm install
```

Install Switchyard using its published Rust binary:

```sh
cargo install --locked switchyard-server
switchyard-server --config config/switchyard.toml --dry-run
```

Add your OpenAI key to `.env`, then load it into the terminal that starts Switchyard:

```sh
set -a
source .env
set +a
switchyard-server --config config/switchyard.toml --host 127.0.0.1 --port 4100
```

Start the API:

```sh
cargo run -p dungeon-router-api
```

Start the frontend in another terminal:

```sh
pnpm dev:web
```

The web client runs at `http://localhost:5173` and proxies `/api` requests to the API at `http://localhost:4000`.

Verify both backend processes before opening the page:

```sh
curl http://127.0.0.1:4100/health
curl http://127.0.0.1:4000/api/health
```

See [Model routing](docs/model-routing.md) to configure and run Switchyard on `http://127.0.0.1:4100`.

The API indexes the bundled SRD automatically on first startup. See [SRD data](docs/srd-data.md) for provenance, rebuild instructions, and search endpoints.

## Checks

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
pnpm typecheck
pnpm build:web
```

## Documentation

- [Project plan](docs/project-plan.md)
- [Progress tracker](docs/progress.md)
- [Architecture](docs/architecture.md)
- [Model routing](docs/model-routing.md)
- [SRD data](docs/srd-data.md)
- [Grounded chat](docs/grounded-chat.md)
- [Campaign notes](docs/campaign-notes.md)
- [Authentication](docs/authentication.md)
- [Single-container deployment](docs/deployment.md)
- [Cost controls](docs/cost-controls.md)
- [Evaluation set](evals/README.md)
- [Known limitations](docs/limitations.md)
- [Demo script](docs/demo-script.md)
- [Contributing](CONTRIBUTING.md)
- [SRD attribution](NOTICE.md)

## License

DungeonRouter source code is licensed under the MIT License. SRD material is separately licensed under CC BY 4.0; see `NOTICE.md` before adding or distributing SRD content.
