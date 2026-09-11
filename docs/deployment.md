# Single-container deployment

DungeonRouter can run as one container for a low-traffic, single-user deployment.
The image contains the compiled React frontend, the Rust API, and Switchyard. The
Rust API serves the frontend, while Supervisor keeps the API and Switchyard
processes running in the same container.

## Coolify configuration

Create a Dockerfile-based application using this repository and expose container
port `4000`.

Set these build arguments:

```text
VITE_SUPABASE_URL=https://your-project.supabase.co
VITE_SUPABASE_ANON_KEY=your-anon-key
```

Set these runtime environment variables:

```text
SUPABASE_URL=https://your-project.supabase.co
SUPABASE_ANON_KEY=your-anon-key
OPENAI_API_KEY=your-openai-key
MONTHLY_COST_WARNING_USD=15
MONTHLY_COST_HARD_LIMIT_USD=20
```

The Dockerfile already sets the internal runtime values:

```text
APP_HOST=0.0.0.0
APP_PORT=4000
DATABASE_URL=sqlite:///app/data/dungeon-router.db?mode=rwc
SWITCHYARD_BASE_URL=http://127.0.0.1:4100
WEB_DIST_DIR=/app/web
```

Add persistent storage at `/app/data`. The SQLite database contains the indexed
SRD, campaign notes, and usage records. Persistent storage is not a backup, so
copy the database or back up the volume separately.

The image health check calls `/api/health`. Coolify should route the domain to
port `4000`; no public port is needed for Switchyard.

## Local image test

```sh
docker build \
  --build-arg VITE_SUPABASE_URL=https://your-project.supabase.co \
  --build-arg VITE_SUPABASE_ANON_KEY=your-anon-key \
  -t dungeon-router .

docker run --rm -p 4000:4000 \
  -e SUPABASE_URL=https://your-project.supabase.co \
  -e SUPABASE_ANON_KEY=your-anon-key \
  -e OPENAI_API_KEY=your-openai-key \
  -v dungeon-router-data:/app/data \
  dungeon-router
```
