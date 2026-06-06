-- SQLite migration 0002: local product state tables.
--
-- Adds the first wave of local product tables: drafts and a sync queue.
-- These are non-authoritative: they hold user-created content and queued
-- offline changes. The source of billing truth is always the cloud backend
-- (ADR-0001).

-- Local user drafts. Any product can write domain-specific content here;
-- the schema intentionally uses generic title/body columns so the platform
-- spine compiles without product-specific types.
CREATE TABLE IF NOT EXISTS local_drafts (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    title       TEXT    NOT NULL DEFAULT '',
    body        TEXT    NOT NULL DEFAULT '',
    created_at  TEXT    NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT    NOT NULL DEFAULT (datetime('now'))
);

-- Sync queue: offline changes waiting to be sent to the backend.
-- status: 'pending' | 'in_flight' | 'failed'
CREATE TABLE IF NOT EXISTS sync_queue (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    entity_type TEXT    NOT NULL,
    entity_id   TEXT    NOT NULL,
    payload     TEXT    NOT NULL,  -- JSON blob
    status      TEXT    NOT NULL DEFAULT 'pending',
    attempts    INTEGER NOT NULL DEFAULT 0,
    last_error  TEXT,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT    NOT NULL DEFAULT (datetime('now'))
);

-- Trigger: keep updated_at current on drafts.
CREATE TRIGGER IF NOT EXISTS local_drafts_updated_at
    AFTER UPDATE ON local_drafts
    FOR EACH ROW
BEGIN
    UPDATE local_drafts SET updated_at = datetime('now') WHERE id = NEW.id;
END;
