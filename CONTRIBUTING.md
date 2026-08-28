# Contributing

DungeonRouter is currently an experimental MVP. Small, focused changes with tests are preferred.

## Development workflow

1. Create a branch from `main`.
2. Keep secrets in `.env`; never commit API keys or campaign notes.
3. Run `cargo fmt --all --check`, the Rust tests, strict Clippy, the frontend typecheck, and the production frontend build before submitting changes.
4. Explain user-visible behavior and cost implications in the pull request.

## Content policy

Only add D&D rules content that the project is permitted to redistribute. SRD 5.1 material must retain its required attribution. Do not commit commercial rulebook text or private campaign notes.
