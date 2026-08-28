use std::env;

use serde::Serialize;
use sqlx::{FromRow, SqlitePool};
use thiserror::Error;

use crate::routing::{RoutingMode, TokenUsage};

pub const PRICING_VERSION: &str = "openai-2026-08-28";

#[derive(Debug, Clone)]
pub struct CostConfig {
    pub nano_input_per_million: f64,
    pub nano_output_per_million: f64,
    pub mini_input_per_million: f64,
    pub mini_output_per_million: f64,
    pub gpt5_input_per_million: f64,
    pub gpt5_output_per_million: f64,
    pub monthly_warning_usd: f64,
    pub monthly_hard_limit_usd: f64,
}

impl Default for CostConfig {
    fn default() -> Self {
        Self {
            nano_input_per_million: 0.05,
            nano_output_per_million: 0.40,
            mini_input_per_million: 0.25,
            mini_output_per_million: 2.00,
            gpt5_input_per_million: 1.25,
            gpt5_output_per_million: 10.00,
            monthly_warning_usd: 15.0,
            monthly_hard_limit_usd: 20.0,
        }
    }
}

impl CostConfig {
    pub fn from_env() -> Result<Self, UsageError> {
        let mut config = Self::default();
        config.nano_input_per_million = env_f64(
            "GPT5_NANO_INPUT_PER_MILLION_USD",
            config.nano_input_per_million,
        )?;
        config.nano_output_per_million = env_f64(
            "GPT5_NANO_OUTPUT_PER_MILLION_USD",
            config.nano_output_per_million,
        )?;
        config.mini_input_per_million = env_f64(
            "GPT5_MINI_INPUT_PER_MILLION_USD",
            config.mini_input_per_million,
        )?;
        config.mini_output_per_million = env_f64(
            "GPT5_MINI_OUTPUT_PER_MILLION_USD",
            config.mini_output_per_million,
        )?;
        config.gpt5_input_per_million =
            env_f64("GPT5_INPUT_PER_MILLION_USD", config.gpt5_input_per_million)?;
        config.gpt5_output_per_million = env_f64(
            "GPT5_OUTPUT_PER_MILLION_USD",
            config.gpt5_output_per_million,
        )?;
        config.monthly_warning_usd = env_f64("MONTHLY_COST_WARNING_USD", 15.0)?;
        config.monthly_hard_limit_usd = env_f64("MONTHLY_COST_HARD_LIMIT_USD", 20.0)?;
        if config.nano_input_per_million < 0.0
            || config.nano_output_per_million < 0.0
            || config.mini_input_per_million < 0.0
            || config.mini_output_per_million < 0.0
            || config.gpt5_input_per_million < 0.0
            || config.gpt5_output_per_million < 0.0
            || config.monthly_warning_usd < 0.0
            || config.monthly_hard_limit_usd <= 0.0
            || config.monthly_warning_usd > config.monthly_hard_limit_usd
        {
            return Err(UsageError::InvalidConfiguration);
        }
        Ok(config)
    }

    pub fn estimate(&self, model: &str, usage: &TokenUsage) -> f64 {
        let (input, output) = if model.contains("nano") {
            (self.nano_input_per_million, self.nano_output_per_million)
        } else if model.contains("mini") {
            (self.mini_input_per_million, self.mini_output_per_million)
        } else {
            (self.gpt5_input_per_million, self.gpt5_output_per_million)
        };
        (usage.input_tokens as f64 * input + usage.output_tokens as f64 * output) / 1_000_000.0
    }

    pub fn gpt5_baseline(&self, usage: &TokenUsage) -> f64 {
        (usage.input_tokens as f64 * self.gpt5_input_per_million
            + usage.output_tokens as f64 * self.gpt5_output_per_million)
            / 1_000_000.0
    }
}

fn env_f64(name: &str, fallback: f64) -> Result<f64, UsageError> {
    match env::var(name) {
        Ok(value) => value.parse().map_err(|_| UsageError::InvalidConfiguration),
        Err(_) => Ok(fallback),
    }
}

#[derive(Debug, Error)]
pub enum UsageError {
    #[error("cost-control configuration is invalid")]
    InvalidConfiguration,
    #[error("the monthly model-cost limit has been reached")]
    BudgetExceeded,
    #[error("usage accounting is unavailable")]
    Unavailable,
}

#[derive(Debug)]
pub struct RunRecord<'a> {
    pub routing_mode: RoutingMode,
    pub selected_model: &'a str,
    pub selection_reason: Option<&'a str>,
    pub classifier_confidence: Option<f64>,
    pub usage: Option<&'a TokenUsage>,
    pub latency_ms: u64,
    pub status: &'a str,
}

pub async fn record(
    pool: &SqlitePool,
    config: &CostConfig,
    run: RunRecord<'_>,
) -> Result<(), UsageError> {
    let empty = TokenUsage {
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 0,
    };
    let tokens = run.usage.unwrap_or(&empty);
    sqlx::query("INSERT INTO model_runs (routing_mode, selected_model, selection_reason, classifier_confidence, input_tokens, output_tokens, estimated_cost_usd, always_gpt5_cost_usd, latency_ms, status, pricing_version) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(match run.routing_mode { RoutingMode::Auto => "auto", RoutingMode::Manual => "manual" })
        .bind(run.selected_model).bind(run.selection_reason).bind(run.classifier_confidence)
        .bind(tokens.input_tokens as i64).bind(tokens.output_tokens as i64)
        .bind(config.estimate(run.selected_model, tokens)).bind(config.gpt5_baseline(tokens))
        .bind(run.latency_ms as i64).bind(run.status).bind(PRICING_VERSION)
        .execute(pool).await.map_err(|_| UsageError::Unavailable)?;
    Ok(())
}

pub async fn enforce_budget(pool: &SqlitePool, config: &CostConfig) -> Result<(), UsageError> {
    let spend: f64 = sqlx::query_scalar("SELECT COALESCE(SUM(estimated_cost_usd), 0.0) FROM model_runs WHERE status = 'completed' AND created_at >= datetime('now', 'start of month')")
        .fetch_one(pool).await.map_err(|_| UsageError::Unavailable)?;
    if spend >= config.monthly_hard_limit_usd {
        Err(UsageError::BudgetExceeded)
    } else {
        Ok(())
    }
}

#[derive(Debug, Serialize, FromRow)]
pub struct ActivityItem {
    pub id: i64,
    pub routing_mode: String,
    pub selected_model: String,
    pub selection_reason: Option<String>,
    pub classifier_confidence: Option<f64>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub estimated_cost_usd: f64,
    pub always_gpt5_cost_usd: f64,
    pub latency_ms: i64,
    pub status: String,
    pub pricing_version: String,
    pub created_at: String,
}

pub async fn activity(pool: &SqlitePool, limit: u32) -> Result<Vec<ActivityItem>, UsageError> {
    sqlx::query_as("SELECT id, routing_mode, selected_model, selection_reason, classifier_confidence, input_tokens, output_tokens, estimated_cost_usd, always_gpt5_cost_usd, latency_ms, status, pricing_version, created_at FROM model_runs ORDER BY id DESC LIMIT ?")
        .bind(i64::from(limit)).fetch_all(pool).await.map_err(|_| UsageError::Unavailable)
}

#[derive(Debug, Serialize, FromRow)]
pub struct ModelCount {
    pub model: String,
    pub requests: i64,
    pub average_latency_ms: f64,
}

#[derive(Debug, Serialize)]
pub struct UsageSummary {
    pub total_questions: i64,
    pub actual_cost_usd: f64,
    pub always_gpt5_cost_usd: f64,
    pub estimated_savings_usd: f64,
    pub estimated_savings_percent: f64,
    pub automatic_requests: i64,
    pub manual_requests: i64,
    pub requests_by_model: Vec<ModelCount>,
    pub warning_threshold_usd: f64,
    pub hard_limit_usd: f64,
    pub budget_status: &'static str,
    pub pricing_version: &'static str,
}

pub async fn summary(pool: &SqlitePool, config: &CostConfig) -> Result<UsageSummary, UsageError> {
    let row: (i64, f64, f64, i64, i64) = sqlx::query_as("SELECT COUNT(*), COALESCE(SUM(estimated_cost_usd), 0.0), COALESCE(SUM(always_gpt5_cost_usd), 0.0), COALESCE(SUM(routing_mode = 'auto'), 0), COALESCE(SUM(routing_mode = 'manual'), 0) FROM model_runs WHERE status = 'completed' AND created_at >= datetime('now', 'start of month')")
        .fetch_one(pool).await.map_err(|_| UsageError::Unavailable)?;
    let by_model = sqlx::query_as::<_, ModelCount>("SELECT selected_model AS model, COUNT(*) AS requests, AVG(latency_ms) AS average_latency_ms FROM model_runs WHERE status = 'completed' AND created_at >= datetime('now', 'start of month') GROUP BY selected_model ORDER BY requests DESC")
        .fetch_all(pool).await.map_err(|_| UsageError::Unavailable)?;
    let savings = (row.2 - row.1).max(0.0);
    Ok(UsageSummary {
        total_questions: row.0,
        actual_cost_usd: row.1,
        always_gpt5_cost_usd: row.2,
        estimated_savings_usd: savings,
        estimated_savings_percent: if row.2 > 0.0 {
            savings / row.2 * 100.0
        } else {
            0.0
        },
        automatic_requests: row.3,
        manual_requests: row.4,
        requests_by_model: by_model,
        warning_threshold_usd: config.monthly_warning_usd,
        hard_limit_usd: config.monthly_hard_limit_usd,
        budget_status: if row.1 >= config.monthly_hard_limit_usd {
            "stopped"
        } else if row.1 >= config.monthly_warning_usd {
            "warning"
        } else {
            "ok"
        },
        pricing_version: PRICING_VERSION,
    })
}

#[derive(Debug, Serialize, FromRow)]
pub struct DailyUsage {
    pub date: String,
    pub questions: i64,
    pub estimated_cost_usd: f64,
}

pub async fn daily(pool: &SqlitePool) -> Result<Vec<DailyUsage>, UsageError> {
    sqlx::query_as("SELECT date(created_at) AS date, COUNT(*) AS questions, SUM(estimated_cost_usd) AS estimated_cost_usd FROM model_runs WHERE status = 'completed' GROUP BY date(created_at) ORDER BY date DESC LIMIT 31")
        .fetch_all(pool).await.map_err(|_| UsageError::Unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;
    #[test]
    fn estimates_each_model_and_gpt5_baseline() {
        let config = CostConfig::default();
        let tokens = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            total_tokens: 2_000_000,
        };
        assert_eq!(config.estimate("gpt-5-nano", &tokens), 0.45);
        assert_eq!(config.estimate("gpt-5-mini", &tokens), 2.25);
        assert_eq!(config.estimate("gpt-5", &tokens), 11.25);
        assert_eq!(config.gpt5_baseline(&tokens), 11.25);
    }

    #[test]
    fn evaluation_set_has_thirty_parseable_cases_and_required_categories() {
        let cases = include_str!("../../../evals/questions.jsonl")
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(cases.len(), 30);
        for category in [
            "direct_lookup",
            "rules_synthesis",
            "complex_adjudication",
            "not_in_srd",
            "campaign_note",
            "adversarial",
        ] {
            assert!(cases.iter().any(|case| case["category"] == category));
        }
    }

    #[tokio::test]
    async fn records_usage_summarizes_savings_and_enforces_hard_limit() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let mut config = CostConfig::default();
        let tokens = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            total_tokens: 2_000_000,
        };
        record(
            &pool,
            &config,
            RunRecord {
                routing_mode: RoutingMode::Auto,
                selected_model: "gpt-5-nano",
                selection_reason: Some("lookup"),
                classifier_confidence: Some(0.9),
                usage: Some(&tokens),
                latency_ms: 50,
                status: "completed",
            },
        )
        .await
        .unwrap();
        let summary = summary(&pool, &config).await.unwrap();
        assert_eq!(summary.total_questions, 1);
        assert_eq!(summary.actual_cost_usd, 0.45);
        assert_eq!(summary.always_gpt5_cost_usd, 11.25);
        assert!(summary.estimated_savings_percent > 90.0);
        config.monthly_hard_limit_usd = 0.40;
        assert!(matches!(
            enforce_budget(&pool, &config).await,
            Err(UsageError::BudgetExceeded)
        ));
    }
}
