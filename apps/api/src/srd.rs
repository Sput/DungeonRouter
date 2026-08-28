use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

pub const SRD_TITLE: &str = "System Reference Document 5.1";
pub const SRD_EDITION: &str = "5e 2014";
pub const SRD_LICENSE: &str = "CC BY 4.0";
pub const SRD_UPSTREAM: &str = "https://github.com/oldmanumby/dnd.srd.5.1";
pub const SRD_REVISION: &str = "cecde944c90b50e630aad031b76af35933805013";
const MAX_CHUNK_CHARS: usize = 1_800;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SrdSnapshot {
    pub title: String,
    pub edition: String,
    pub license: String,
    pub upstream: String,
    pub revision: String,
    pub documents: Vec<SrdDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SrdDocument {
    pub title: String,
    pub source_path: String,
    pub chunks: Vec<SrdChunk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SrdChunk {
    pub heading: String,
    pub section_path: String,
    pub content: String,
    pub ordinal: usize,
    pub source_locator: String,
}

pub fn build_snapshot(root: &Path) -> Result<SrdSnapshot> {
    if !root.join("Legal.md").is_file() {
        bail!("source directory does not contain Legal.md");
    }
    let legal = fs::read_to_string(root.join("Legal.md")).context("failed to read Legal.md")?;
    if !legal.contains("Creative Commons Attribution 4.0")
        || !legal.contains("System Reference Document 5.1")
    {
        bail!("source directory does not contain the expected SRD 5.1 CC attribution");
    }

    let mut paths = Vec::new();
    collect_markdown_paths(root, root, &mut paths)?;
    paths.sort();
    let documents = paths
        .into_iter()
        .map(|path| parse_document(root, &path))
        .collect::<Result<Vec<_>>>()?;

    Ok(SrdSnapshot {
        title: SRD_TITLE.into(),
        edition: SRD_EDITION.into(),
        license: SRD_LICENSE.into(),
        upstream: SRD_UPSTREAM.into(),
        revision: SRD_REVISION.into(),
        documents,
    })
}

fn collect_markdown_paths(root: &Path, directory: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(directory)
        .with_context(|| format!("failed to read {}", directory.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy();
            if relative.contains("Spells_A-Z")
                || relative.contains("Monsters_A-Z")
                || relative.starts_with('.')
            {
                continue;
            }
            collect_markdown_paths(root, &path, paths)?;
        } else if path.extension().and_then(|value| value.to_str()) == Some("md") {
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            if !matches!(name, "README.md" | "Legal.md") {
                paths.push(path);
            }
        }
    }
    Ok(())
}

fn parse_document(root: &Path, path: &Path) -> Result<SrdDocument> {
    let source_path = path
        .strip_prefix(root)
        .context("source file is outside root")?
        .to_string_lossy()
        .replace('\\', "/");
    let markdown =
        fs::read_to_string(path).with_context(|| format!("failed to read {source_path}"))?;
    let title = markdown
        .lines()
        .find_map(|line| line.strip_prefix("# "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            path.file_stem()
                .and_then(|value| value.to_str())
                .map(|value| value.replace('_', " "))
        })
        .unwrap_or_else(|| source_path.clone());
    let chunks = chunk_markdown(&markdown, &source_path);
    Ok(SrdDocument {
        title,
        source_path,
        chunks,
    })
}

pub fn chunk_markdown(markdown: &str, source_path: &str) -> Vec<SrdChunk> {
    let mut headings: Vec<String> = Vec::new();
    let mut sections: Vec<(String, String)> = Vec::new();
    let mut body = String::new();

    let flush = |headings: &[String], body: &mut String, sections: &mut Vec<(String, String)>| {
        let content = body.trim();
        if !content.is_empty() {
            let section_path = if headings.is_empty() {
                "Overview".into()
            } else {
                headings.join(" > ")
            };
            sections.push((section_path, content.to_owned()));
        }
        body.clear();
    };

    for line in markdown.lines() {
        if let Some((level, heading)) = markdown_heading(line) {
            flush(&headings, &mut body, &mut sections);
            headings.truncate(level.saturating_sub(1));
            headings.push(heading.to_owned());
        } else {
            body.push_str(line.trim_end());
            body.push('\n');
        }
    }
    flush(&headings, &mut body, &mut sections);

    let mut chunks = Vec::new();
    for (section_path, content) in sections {
        let heading = section_path
            .rsplit(" > ")
            .next()
            .unwrap_or("Overview")
            .to_owned();
        for piece in split_content(&content, MAX_CHUNK_CHARS) {
            let ordinal = chunks.len();
            chunks.push(SrdChunk {
                heading: heading.clone(),
                section_path: section_path.clone(),
                content: piece,
                ordinal,
                source_locator: format!("{source_path}#{}", slugify(&heading)),
            });
        }
    }
    chunks
}

fn markdown_heading(line: &str) -> Option<(usize, &str)> {
    let hashes = line.bytes().take_while(|byte| *byte == b'#').count();
    if !(1..=6).contains(&hashes) || line.as_bytes().get(hashes) != Some(&b' ') {
        return None;
    }
    let heading = line[hashes + 1..].trim().trim_end_matches('#').trim();
    (!heading.is_empty()).then_some((hashes, heading))
}

fn split_content(content: &str, max_chars: usize) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    for paragraph in content
        .split("\n\n")
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if paragraph.chars().count() > max_chars {
            if !current.is_empty() {
                result.push(current.trim().to_owned());
                current.clear();
            }
            let words = paragraph.split_whitespace();
            for word in words {
                if current.chars().count() + word.chars().count() + 1 > max_chars
                    && !current.is_empty()
                {
                    result.push(current.trim().to_owned());
                    current.clear();
                }
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(word);
            }
        } else {
            let separator = usize::from(!current.is_empty()) * 2;
            if current.chars().count() + paragraph.chars().count() + separator > max_chars
                && !current.is_empty()
            {
                result.push(current.trim().to_owned());
                current.clear();
            }
            if !current.is_empty() {
                current.push_str("\n\n");
            }
            current.push_str(paragraph);
        }
    }
    if !current.is_empty() {
        result.push(current.trim().to_owned());
    }
    result
}

fn slugify(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .map(|character| {
            if character.is_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

pub async fn ingest_snapshot(pool: &SqlitePool, snapshot: &SrdSnapshot) -> Result<usize> {
    let mut transaction = pool.begin().await?;
    sqlx::query("DELETE FROM documents WHERE kind = 'srd'")
        .execute(&mut *transaction)
        .await?;
    let mut chunk_count = 0;
    for document in &snapshot.documents {
        let document_id: i64 = sqlx::query_scalar(
            "INSERT INTO documents (kind, title, source_path, source_url, source_revision, edition, license) VALUES ('srd', ?, ?, ?, ?, ?, ?) RETURNING id"
        ).bind(&document.title).bind(&document.source_path).bind(&snapshot.upstream).bind(&snapshot.revision).bind(&snapshot.edition).bind(&snapshot.license)
            .fetch_one(&mut *transaction).await?;
        insert_chunks(&mut transaction, document_id, &document.chunks).await?;
        chunk_count += document.chunks.len();
    }
    transaction.commit().await?;
    Ok(chunk_count)
}

async fn insert_chunks(
    transaction: &mut Transaction<'_, Sqlite>,
    document_id: i64,
    chunks: &[SrdChunk],
) -> Result<()> {
    for chunk in chunks {
        sqlx::query("INSERT INTO source_chunks (document_id, heading, section_path, content, ordinal, source_locator) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(document_id).bind(&chunk.heading).bind(&chunk.section_path).bind(&chunk.content)
            .bind(chunk.ordinal as i64).bind(&chunk.source_locator).execute(&mut **transaction).await?;
    }
    Ok(())
}

pub async fn ensure_bundled_srd(pool: &SqlitePool) -> Result<usize> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM documents WHERE kind = 'srd'")
        .fetch_one(pool)
        .await?;
    if count > 0 {
        return Ok(0);
    }
    let snapshot: SrdSnapshot = serde_json::from_str(include_str!("../../../content/srd-5.1.json"))
        .context("bundled SRD snapshot is invalid")?;
    ingest_snapshot(pool, &snapshot).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_is_split_on_hierarchical_headings() {
        let chunks = chunk_markdown(
            "# Conditions\nIntro.\n\n## Prone\nA prone creature crawls.\n",
            "Rules/Conditions.md",
        );
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[1].heading, "Prone");
        assert_eq!(chunks[1].section_path, "Conditions > Prone");
        assert_eq!(chunks[1].source_locator, "Rules/Conditions.md#prone");
    }

    #[test]
    fn oversized_sections_are_bounded() {
        let text = format!("# Long Rule\n{}", "word ".repeat(900));
        let chunks = chunk_markdown(&text, "Rules/Long.md");
        assert!(chunks.len() > 1);
        assert!(
            chunks
                .iter()
                .all(|chunk| chunk.content.chars().count() <= MAX_CHUNK_CHARS)
        );
    }
}
