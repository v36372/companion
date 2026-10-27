-- crates/companion-store/src/migrations/0001_init.sql
PRAGMA foreign_keys = ON;

CREATE TABLE schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE TABLE documents (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    source_path TEXT NOT NULL,
    stored_path TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    page_count INTEGER,
    added_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    last_opened_at TEXT
);
CREATE UNIQUE INDEX documents_sha256 ON documents(sha256);
CREATE INDEX documents_last_opened ON documents(last_opened_at DESC);

CREATE TABLE highlights (
    id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    page INTEGER NOT NULL,              -- 0-based
    quote_text TEXT NOT NULL,
    occurrence INTEGER NOT NULL DEFAULT 0,
    color TEXT NOT NULL DEFAULT 'yellow',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    conversation_entry_id INTEGER,
    UNIQUE (document_id, page, quote_text, occurrence)  -- one color per quote (v1 limit)
);
CREATE INDEX highlights_document ON highlights(document_id, page);

CREATE TABLE reading_progress (
    document_id TEXT PRIMARY KEY REFERENCES documents(id) ON DELETE CASCADE,
    page INTEGER NOT NULL DEFAULT 0,    -- 0-based, last explicit nav
    scroll_frac REAL NOT NULL DEFAULT 0,
    viewport_excerpt TEXT,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE TABLE conversations_map (
    document_id TEXT PRIMARY KEY REFERENCES documents(id) ON DELETE CASCADE,
    conversation_id INTEGER NOT NULL
);
CREATE INDEX conversations_map_conv ON conversations_map(conversation_id);

CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL
);

CREATE TABLE notes (
    document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    content BLOB NOT NULL,
    mtime_ms INTEGER NOT NULL,
    PRIMARY KEY (document_id, path)
);
