use serde::Serialize;
use sqlx::{FromRow, SqlitePool};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct SearchResult {
    pub chunk_id: i64,
    pub document_title: String,
    pub heading: String,
    pub section_path: String,
    pub excerpt: String,
    pub source_locator: String,
    pub source_url: Option<String>,
    pub source_revision: Option<String>,
    pub license: Option<String>,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct SourceChunk {
    pub chunk_id: i64,
    pub document_title: String,
    pub heading: String,
    pub section_path: String,
    pub content: String,
    pub source_locator: String,
    pub source_url: Option<String>,
    pub source_revision: Option<String>,
    pub license: Option<String>,
}

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("search query must contain a meaningful word")]
    EmptyQuery,
    #[error("source passage was not found")]
    NotFound,
    #[error("rules search is unavailable")]
    Unavailable,
}

pub async fn search(
    pool: &SqlitePool,
    query: &str,
    limit: u32,
) -> Result<Vec<SearchResult>, SearchError> {
    let expression = fts_expression(query).ok_or(SearchError::EmptyQuery)?;
    sqlx::query_as::<_, SearchResult>(
        r#"
        SELECT
            source_chunks.id AS chunk_id,
            documents.title AS document_title,
            source_chunks.heading,
            source_chunks.section_path,
            snippet(source_chunks_fts, 2, '', '', ' … ', 32) AS excerpt,
            source_chunks.source_locator,
            documents.source_url,
            documents.source_revision,
            documents.license,
            -bm25(source_chunks_fts, 3.0, 1.5, 1.0) AS score
        FROM source_chunks_fts
        JOIN source_chunks ON source_chunks.id = source_chunks_fts.rowid
        JOIN documents ON documents.id = source_chunks.document_id
        WHERE source_chunks_fts MATCH ?
        ORDER BY bm25(source_chunks_fts, 3.0, 1.5, 1.0)
        LIMIT ?
        "#,
    )
    .bind(expression)
    .bind(i64::from(limit))
    .fetch_all(pool)
    .await
    .map_err(|_| SearchError::Unavailable)
}

pub async fn source(pool: &SqlitePool, chunk_id: i64) -> Result<SourceChunk, SearchError> {
    sqlx::query_as::<_, SourceChunk>(
        r#"
        SELECT source_chunks.id AS chunk_id, documents.title AS document_title,
            source_chunks.heading, source_chunks.section_path, source_chunks.content,
            source_chunks.source_locator, documents.source_url, documents.source_revision, documents.license
        FROM source_chunks JOIN documents ON documents.id = source_chunks.document_id
        WHERE source_chunks.id = ?
        "#,
    )
    .bind(chunk_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| SearchError::Unavailable)?
    .ok_or(SearchError::NotFound)
}

fn fts_expression(query: &str) -> Option<String> {
    const STOP_WORDS: &[&str] = &[
        "a", "an", "and", "are", "can", "do", "does", "for", "how", "i", "in", "is", "it", "of",
        "on", "the", "to", "what", "when", "with",
    ];
    let mut terms = query
        .split(|character: char| !character.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|term| term.len() > 1 && !STOP_WORDS.contains(&term.as_str()))
        .take(8)
        .collect::<Vec<_>>();
    terms.sort();
    terms.dedup();
    (!terms.is_empty()).then(|| {
        terms
            .into_iter()
            .map(|term| format!("\"{term}\"*"))
            .collect::<Vec<_>>()
            .join(" AND ")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::srd::{SrdChunk, SrdDocument, SrdSnapshot, ingest_snapshot};
    use sqlx::sqlite::SqlitePoolOptions;

    async fn fixture_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let snapshot = SrdSnapshot {
            title: "SRD".into(),
            edition: "5e 2014".into(),
            license: "CC BY 4.0".into(),
            upstream: "example".into(),
            revision: "abc123".into(),
            documents: vec![SrdDocument {
                title: "Conditions".into(),
                source_path: "Conditions.md".into(),
                chunks: vec![SrdChunk {
                    heading: "Prone".into(),
                    section_path: "Conditions > Prone".into(),
                    content:
                        "A prone creature's only movement option is to crawl unless it stands up."
                            .into(),
                    ordinal: 0,
                    source_locator: "Conditions.md#prone".into(),
                }],
            }],
        };
        ingest_snapshot(&pool, &snapshot).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn finds_rules_with_citation_metadata() {
        let results = search(&fixture_pool().await, "What does prone do?", 5)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].heading, "Prone");
        assert_eq!(results[0].source_revision.as_deref(), Some("abc123"));
        assert!(results[0].excerpt.contains("prone"));
    }

    #[tokio::test]
    async fn source_returns_complete_passage() {
        let pool = fixture_pool().await;
        let result = search(&pool, "prone", 1).await.unwrap();
        let passage = source(&pool, result[0].chunk_id).await.unwrap();
        assert!(passage.content.contains("only movement option"));
    }

    #[test]
    fn query_syntax_is_sanitized() {
        assert_eq!(fts_expression("what is prone?"), Some("\"prone\"*".into()));
        assert_eq!(fts_expression("the and what"), None);
    }
}
