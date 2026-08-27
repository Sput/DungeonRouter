# DungeonRouter

DungeonRouter is an experimental, open-source D&D 5e (2014) rules assistant. It is designed to search SRD 5.1 and campaign notes, cite its sources, and demonstrate cost-aware routing across `gpt-5-nano`, `gpt-5-mini`, and `gpt-5` through NVIDIA NeMo Switchyard.

The repository currently contains the Task 1 application foundation:

- a Rust/Axum API;
- SQLite migrations and startup initialization;
- a React/TypeScript/Vite frontend;
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

Switchyard and an OpenAI API key are not required until the routing milestone.

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

## Checks

```sh
cargo test --workspace
pnpm typecheck
pnpm build:web
```

## Documentation

- [Project plan](docs/project-plan.md)
- [Contributing](CONTRIBUTING.md)
- [SRD attribution](NOTICE.md)

## License

DungeonRouter source code is licensed under the MIT License. SRD material is separately licensed under CC BY 4.0; see `NOTICE.md` before adding or distributing SRD content.

