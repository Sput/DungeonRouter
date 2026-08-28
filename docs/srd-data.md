# SRD data

DungeonRouter bundles a normalized, search-ready snapshot of SRD 5.1. This keeps the demo deterministic and allows rules retrieval without a network request or embedding cost.

## Provenance and license

- Canonical rules document: System Reference Document 5.1
- License: Creative Commons Attribution 4.0 International
- Markdown adaptation: <https://github.com/oldmanumby/dnd.srd.5.1>
- Pinned upstream revision: `cecde944c90b50e630aad031b76af35933805013`
- Bundled snapshot: `content/srd-5.1.json`

The required attribution is preserved in `NOTICE.md`. The upstream adaptation is convenient structured input; the official SRD remains authoritative if a transcription discrepancy is discovered.

## Normalization

The snapshot builder:

1. verifies that the source contains the expected SRD 5.1 CC attribution;
2. recursively reads Markdown documents in stable path order;
3. excludes repository metadata, documentation, images, and branding;
4. excludes `Spells_A-Z` and `Monsters_A-Z` because the same entries exist in the canonical `Spells_Each` and `Monsters_Each` directories;
5. preserves the Markdown heading hierarchy;
6. creates chunks no larger than 1,800 characters;
7. records upstream path, heading anchor, license, and pinned revision for citations.

The generated snapshot contains 958 documents and 2,527 chunks and is approximately 2.1 MB.

## Rebuild the snapshot

Clone or check out the pinned upstream revision, then run:

```sh
cargo run -p dungeon-router-api --bin build_srd_snapshot -- \
  /path/to/dnd.srd.5.1 \
  content/srd-5.1.json
```

Review the generated diff and run the full test suite before accepting an update. Changing the pinned revision requires updating `SRD_REVISION`, `NOTICE.md`, and this document.

## Database ingestion

The API automatically imports the bundled snapshot when the database has no SRD documents. To rebuild an existing database explicitly:

```sh
cargo run -p dungeon-router-api --bin ingest_srd -- content/srd-5.1.json
```

Importing is transactional and replaces only documents whose kind is `srd`; future campaign notes are left untouched.

## Search API

```text
GET /api/search?q=prone+condition&limit=6
GET /api/sources/{chunk_id}
```

Search uses SQLite FTS5 with extra weight on headings. Results contain an excerpt, full section path, upstream locator, revision, license, and a stable local chunk ID. The source endpoint returns the complete passage selected by that ID.
