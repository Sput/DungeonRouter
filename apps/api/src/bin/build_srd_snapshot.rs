use std::{env, fs, path::PathBuf};

use anyhow::{Context, Result};
use dungeon_router_api::srd::build_snapshot;

fn main() -> Result<()> {
    let mut arguments = env::args_os().skip(1);
    let source = arguments
        .next()
        .map(PathBuf::from)
        .context("usage: build-srd-snapshot <markdown-repository> [output.json]")?;
    let output = arguments
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("content/srd-5.1.json"));
    let snapshot = build_snapshot(&source)?;
    let document_count = snapshot.documents.len();
    let chunk_count: usize = snapshot
        .documents
        .iter()
        .map(|document| document.chunks.len())
        .sum();
    let json = serde_json::to_vec(&snapshot).context("failed to serialize SRD snapshot")?;
    fs::write(&output, json).with_context(|| format!("failed to write {}", output.display()))?;
    println!(
        "Wrote {document_count} documents and {chunk_count} chunks to {}",
        output.display()
    );
    Ok(())
}
