CREATE TABLE IF NOT EXISTS documents (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL CHECK (kind IN ('srd', 'campaign_note')),
    title TEXT NOT NULL,
    source_path TEXT NOT NULL,
    source_url TEXT,
    source_revision TEXT,
    edition TEXT,
    license TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(kind, source_path)
);

CREATE TABLE IF NOT EXISTS source_chunks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    heading TEXT NOT NULL,
    section_path TEXT NOT NULL,
    content TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    source_locator TEXT NOT NULL,
    FOREIGN KEY(document_id) REFERENCES documents(id),
    UNIQUE(document_id, ordinal)
);

CREATE VIRTUAL TABLE IF NOT EXISTS source_chunks_fts USING fts5(
    heading,
    section_path,
    content,
    content='source_chunks',
    content_rowid='id',
    tokenize='unicode61 remove_diacritics 2'
);

CREATE TRIGGER IF NOT EXISTS source_chunks_ai AFTER INSERT ON source_chunks BEGIN
    INSERT INTO source_chunks_fts(rowid, heading, section_path, content)
    VALUES (new.id, new.heading, new.section_path, new.content);
END;

CREATE TRIGGER IF NOT EXISTS source_chunks_ad AFTER DELETE ON source_chunks BEGIN
    INSERT INTO source_chunks_fts(source_chunks_fts, rowid, heading, section_path, content)
    VALUES ('delete', old.id, old.heading, old.section_path, old.content);
END;

CREATE TRIGGER IF NOT EXISTS source_chunks_au AFTER UPDATE ON source_chunks BEGIN
    INSERT INTO source_chunks_fts(source_chunks_fts, rowid, heading, section_path, content)
    VALUES ('delete', old.id, old.heading, old.section_path, old.content);
    INSERT INTO source_chunks_fts(rowid, heading, section_path, content)
    VALUES (new.id, new.heading, new.section_path, new.content);
END;

CREATE INDEX IF NOT EXISTS source_chunks_document_id_idx ON source_chunks(document_id);
