use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use thiserror::Error;

use crate::srd::chunk_markdown;

const MAX_NOTE_BYTES: usize = 256 * 1024;

#[derive(Debug, Deserialize)]
pub struct CreateNote {
    pub title: String,
    pub filename: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct NoteSummary {
    pub id: i64,
    pub title: String,
    pub filename: String,
    pub chunk_count: i64,
    pub created_at: String,
}

#[derive(Debug, Error)]
pub enum NoteError {
    #[error("note title must be between 1 and 120 characters")]
    InvalidTitle,
    #[error("only .md, .markdown, and .txt notes are supported")]
    InvalidFileType,
    #[error("note must contain text and be no larger than 256 KiB")]
    InvalidContent,
    #[error("a note with this filename already exists")]
    Duplicate,
    #[error("campaign note was not found")]
    NotFound,
    #[error("campaign-note storage is unavailable")]
    Unavailable,
}

pub async fn create(pool: &SqlitePool, request: CreateNote) -> Result<NoteSummary, NoteError> {
    let title = request.title.trim();
    if title.is_empty() || title.chars().count() > 120 {
        return Err(NoteError::InvalidTitle);
    }
    let filename = request.filename.trim();
    let safe_name = std::path::Path::new(filename)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| *value == filename)
        .ok_or(NoteError::InvalidFileType)?;
    let extension = std::path::Path::new(safe_name)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase);
    if !matches!(extension.as_deref(), Some("md" | "markdown" | "txt")) {
        return Err(NoteError::InvalidFileType);
    }
    let content = request.content.trim();
    if content.is_empty() || request.content.len() > MAX_NOTE_BYTES {
        return Err(NoteError::InvalidContent);
    }
    let chunks = chunk_markdown(content, safe_name);
    if chunks.is_empty() {
        return Err(NoteError::InvalidContent);
    }

    let mut transaction = pool.begin().await.map_err(|_| NoteError::Unavailable)?;
    let document_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO documents (kind, title, source_path) VALUES ('campaign_note', ?, ?) RETURNING id",
    )
    .bind(title)
    .bind(safe_name)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| {
        if error.to_string().contains("UNIQUE constraint failed") {
            NoteError::Duplicate
        } else {
            NoteError::Unavailable
        }
    })?;

    for chunk in &chunks {
        sqlx::query("INSERT INTO source_chunks (document_id, heading, section_path, content, ordinal, source_locator) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(document_id)
            .bind(&chunk.heading)
            .bind(&chunk.section_path)
            .bind(&chunk.content)
            .bind(chunk.ordinal as i64)
            .bind(&chunk.source_locator)
            .execute(&mut *transaction)
            .await
            .map_err(|_| NoteError::Unavailable)?;
    }
    transaction
        .commit()
        .await
        .map_err(|_| NoteError::Unavailable)?;
    get(pool, document_id).await
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<NoteSummary>, NoteError> {
    sqlx::query_as::<_, NoteSummary>(
        r#"SELECT documents.id, documents.title, documents.source_path AS filename,
        COUNT(source_chunks.id) AS chunk_count, documents.created_at
        FROM documents LEFT JOIN source_chunks ON source_chunks.document_id = documents.id
        WHERE documents.kind = 'campaign_note'
        GROUP BY documents.id ORDER BY documents.created_at DESC, documents.id DESC"#,
    )
    .fetch_all(pool)
    .await
    .map_err(|_| NoteError::Unavailable)
}

pub async fn get(pool: &SqlitePool, id: i64) -> Result<NoteSummary, NoteError> {
    sqlx::query_as::<_, NoteSummary>(
        r#"SELECT documents.id, documents.title, documents.source_path AS filename,
        COUNT(source_chunks.id) AS chunk_count, documents.created_at
        FROM documents LEFT JOIN source_chunks ON source_chunks.document_id = documents.id
        WHERE documents.kind = 'campaign_note' AND documents.id = ? GROUP BY documents.id"#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|_| NoteError::Unavailable)?
    .ok_or(NoteError::NotFound)
}

pub async fn delete(pool: &SqlitePool, id: i64) -> Result<(), NoteError> {
    let mut transaction = pool.begin().await.map_err(|_| NoteError::Unavailable)?;
    sqlx::query("DELETE FROM source_chunks WHERE document_id = ?")
        .bind(id)
        .execute(&mut *transaction)
        .await
        .map_err(|_| NoteError::Unavailable)?;
    let result = sqlx::query("DELETE FROM documents WHERE id = ? AND kind = 'campaign_note'")
        .bind(id)
        .execute(&mut *transaction)
        .await
        .map_err(|_| NoteError::Unavailable)?;
    if result.rows_affected() == 0 {
        return Err(NoteError::NotFound);
    }
    transaction
        .commit()
        .await
        .map_err(|_| NoteError::Unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn note_lifecycle_updates_full_text_search() {
        let pool = pool().await;
        let note = create(
            &pool,
            CreateNote {
                title: "The Ashen Vale".into(),
                filename: "ashen-vale.md".into(),
                content: "# House Rules\n\nHealing potions use a bonus action.".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(note.chunk_count, 1);
        assert_eq!(list(&pool).await.unwrap().len(), 1);

        let matches = search::search(&pool, "healing potion bonus action", 5)
            .await
            .unwrap();
        assert_eq!(matches[0].kind, "campaign_note");
        assert_eq!(matches[0].document_title, "The Ashen Vale");

        delete(&pool, note.id).await.unwrap();
        assert!(list(&pool).await.unwrap().is_empty());
        assert!(
            search::search(&pool, "healing potion bonus action", 5)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn rejects_unsupported_files() {
        let error = create(
            &pool().await,
            CreateNote {
                title: "Book".into(),
                filename: "players-handbook.pdf".into(),
                content: "not accepted".into(),
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(error, NoteError::InvalidFileType));
    }
}
