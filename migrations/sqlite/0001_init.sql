-- SQLite bootstrap migration (local product state).
--
-- Walking skeleton: establishes the migration tracking table only. Local
-- product tables (drafts, sync queue, cached reads, local indexes) land with
-- the product modules. Local SQLite never stores authoritative billing or
-- subscription truth (ADR-0001).

CREATE TABLE IF NOT EXISTS schema_migrations (
    version     TEXT PRIMARY KEY,
    applied_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
