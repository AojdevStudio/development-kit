//! The Notes sample product — desktop/local half (issue #37).
//!
//! Notes plugs its local SQLite state into the kit through the seam's
//! [`LocalModule`](crate::product_module::LocalModule): [`NotesLocal`] returns the
//! product [`ProductModuleMeta`] and the [`Migration`]s for its `notes_*` tables,
//! and `crate::product_module::apply_module` runs them additively after the
//! baseline schema — touching no baseline migration.
//!
//! Domain: a note has a title and a body. Creating and listing notes locally is a
//! FREE local action — this module is the authoritative store for that local
//! product state (ADR-0001: local SQLite is authoritative for local product work,
//! never for billing). Publishing a note to the cloud is the PAID action, gated
//! server-side by the backend half (`api::products::notes`); the desktop's Tauri
//! command enforces the same gate locally before it would enqueue a publish.

use rusqlite::Connection;
use shared::ProductModuleMeta;

use crate::error::Result;
use crate::migration::Migration;
use crate::product_module::LocalModule;

/// The Notes product namespace. Matches the backend half so routes, keys, and
/// tables all derive from the one value (`notes`).
pub const NAMESPACE: &str = "notes";

/// The Notes local migrations: one table, `notes_notes`, named with the
/// `<namespace>_` prefix the seam convention requires. The migration version is
/// `notes_NNNN_<desc>` so it never collides with another product's versions in
/// the shared `schema_migrations` ledger.
const NOTES_MIGRATIONS: &[Migration] = &[Migration {
    version: "notes_0001_init",
    sql: "CREATE TABLE IF NOT EXISTS notes_notes (
            id          TEXT PRIMARY KEY,
            title       TEXT NOT NULL DEFAULT '',
            body        TEXT NOT NULL DEFAULT '',
            created_at  TEXT NOT NULL DEFAULT (datetime('now'))
        );",
}];

/// The Notes [`LocalModule`]: its identity and its SQLite migrations.
pub struct NotesLocal;

impl LocalModule for NotesLocal {
    fn meta(&self) -> ProductModuleMeta {
        ProductModuleMeta::new("Notes", NAMESPACE).expect("valid notes namespace")
    }

    fn migrations(&self) -> &'static [Migration] {
        NOTES_MIGRATIONS
    }
}

/// A locally-stored note — the FREE product state. Title + body are user content;
/// the local store is authoritative for it (ADR-0001), never for billing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub id: String,
    pub title: String,
    pub body: String,
}

/// Insert a note into the local store (the FREE create action). Pure over the
/// connection and the note, so it is testable against an in-memory DB. Idempotent
/// per id is NOT promised — a duplicate id is a primary-key conflict the caller
/// surfaces, matching the spine's typed-error discipline.
pub fn create_note(conn: &Connection, note: &Note) -> Result<()> {
    conn.execute(
        "INSERT INTO notes_notes (id, title, body) VALUES (?1, ?2, ?3)",
        (&note.id, &note.title, &note.body),
    )?;
    Ok(())
}

/// List all locally-stored notes, most recent first (the FREE list action).
pub fn list_notes(conn: &Connection) -> Result<Vec<Note>> {
    let mut stmt =
        conn.prepare("SELECT id, title, body FROM notes_notes ORDER BY created_at DESC, id DESC")?;
    let rows = stmt.query_map([], |row| {
        Ok(Note {
            id: row.get(0)?,
            title: row.get(1)?,
            body: row.get(2)?,
        })
    })?;
    let mut notes = Vec::new();
    for note in rows {
        notes.push(note?);
    }
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration::MIGRATIONS;
    use crate::product_module::apply_module;

    fn memory() -> Connection {
        Connection::open_in_memory().expect("in-memory DB")
    }

    // --- ISC-37-3: the Notes migration applies additively on top of the baseline ---
    #[test]
    fn applying_notes_runs_baseline_then_notes_migrations() {
        let conn = memory();
        let applied = apply_module(&conn, &NotesLocal).unwrap();
        assert_eq!(applied, MIGRATIONS.len() + NOTES_MIGRATIONS.len());
    }

    // --- ISC-37-4: the Notes table is named with the namespace prefix ---
    #[test]
    fn notes_table_uses_the_namespace_prefix() {
        let prefix = NotesLocal.meta().table_prefix();
        assert!(NOTES_MIGRATIONS[0].sql.contains(&format!("{prefix}notes")));
    }
}
