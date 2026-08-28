CREATE TABLE IF NOT EXISTS model_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    routing_mode TEXT NOT NULL CHECK (routing_mode IN ('auto', 'manual')),
    selected_model TEXT NOT NULL,
    selection_reason TEXT,
    classifier_confidence REAL,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    estimated_cost_usd REAL NOT NULL DEFAULT 0,
    always_gpt5_cost_usd REAL NOT NULL DEFAULT 0,
    latency_ms INTEGER NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('completed', 'failed')),
    pricing_version TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS model_runs_created_at_idx ON model_runs(created_at);
CREATE INDEX IF NOT EXISTS model_runs_selected_model_idx ON model_runs(selected_model);
