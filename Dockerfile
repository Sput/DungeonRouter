FROM node:20-bookworm-slim AS web-builder

WORKDIR /app
RUN corepack enable

COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY apps/web/package.json apps/web/package.json
RUN pnpm install --frozen-lockfile

COPY apps/web apps/web
ARG VITE_SUPABASE_URL
ARG VITE_SUPABASE_ANON_KEY
ENV VITE_SUPABASE_URL=$VITE_SUPABASE_URL
ENV VITE_SUPABASE_ANON_KEY=$VITE_SUPABASE_ANON_KEY
RUN pnpm build:web

FROM rust:1.88-bookworm AS rust-builder

WORKDIR /app
COPY Cargo.toml Cargo.lock rustfmt.toml ./
COPY apps/api apps/api
COPY content content
RUN cargo build --release -p dungeon-router-api
RUN cargo install --locked switchyard-server

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install --no-install-recommends -y ca-certificates curl supervisor \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=rust-builder /app/target/release/dungeon-router-api /usr/local/bin/dungeon-router-api
COPY --from=rust-builder /usr/local/cargo/bin/switchyard-server /usr/local/bin/switchyard-server
COPY --from=web-builder /app/apps/web/dist ./web
COPY apps/api/migrations ./migrations
COPY config/switchyard.toml ./config/switchyard.toml
COPY content/srd-5.1.json ./content/srd-5.1.json
COPY docker/supervisord.conf /etc/supervisor/conf.d/dungeon-router.conf

RUN mkdir -p /app/data

ENV APP_HOST=0.0.0.0 \
    APP_PORT=4000 \
    DATABASE_URL=sqlite:///app/data/dungeon-router.db?mode=rwc \
    SWITCHYARD_BASE_URL=http://127.0.0.1:4100 \
    WEB_DIST_DIR=/app/web

EXPOSE 4000
VOLUME ["/app/data"]
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD curl --fail http://127.0.0.1:4000/api/health || exit 1

CMD ["/usr/bin/supervisord", "-n", "-c", "/etc/supervisor/supervisord.conf"]
