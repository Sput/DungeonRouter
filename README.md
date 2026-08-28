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

An OpenAI API key is not required for unit tests or frontend development. It is required only when exercising live model calls through Switchyard.

## Local development

Copy the environment template:

```sh
cp .env.example .env
```

Install frontend dependencies:

```sh
pnpm install
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
- [Model routing](docs/model-routing.md)
- [SRD data](docs/srd-data.md)
- [Grounded chat](docs/grounded-chat.md)
- [Contributing](CONTRIBUTING.md)
- [SRD attribution](NOTICE.md)

## License

DungeonRouter source code is licensed under the MIT License. SRD material is separately licensed under CC BY 4.0; see `NOTICE.md` before adding or distributing SRD content.
