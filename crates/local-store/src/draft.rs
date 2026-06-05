//! DraftRepository — typed access to the `local_drafts` table.
//!
//! Drafts are the canonical example of "local product state that survives
//! restarts and offline use." They are user-created, locally stored, and
//! never the source of truth for billing (ADR-0001).

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::{LocalStoreError, Result};

/// A locally stored draft document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    pub id: i64,
    pub title: String,
    pub body: String,
}

/// Typed repository for `local_drafts`.
///
/// Borrows the connection; the caller owns the `LocalDb` and passes
/// `&LocalDb::conn` (or wraps repositories in a service layer).
pub struct DraftRepository<'conn> {
    conn: &'conn Connection,
}

impl<'conn> DraftRepository<'conn> {
    pub fn new(conn: &'conn Connection) -> Self {
        DraftRepository { conn }
    }

    /// Insert a new draft and return the row id assigned by SQLite.
    pub fn save(&self, title: &str, body: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO local_drafts (title, body) VALUES (?1, ?2)",
            params![title, body],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Retrieve a draft by its id. Returns `Err(NotFound)` if absent.
    pub fn find(&self, id: i64) -> Result<Draft> {
        self.conn
            .query_row(
                "SELECT id, title, body FROM local_drafts WHERE id = ?1",
                params![id],
                |row| {
                    Ok(Draft {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        body: row.get(2)?,
                    })
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    LocalStoreError::NotFound(format!("draft id={id}"))
                }
                other => LocalStoreError::Database(other),
            })
    }

    /// Return all drafts ordered by id ascending.
    pub fn list(&self) -> Result<Vec<Draft>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, title, body FROM local_drafts ORDER BY id")?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Draft {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    body: row.get(2)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Update the title and body of an existing draft. Returns `Err(NotFound)`
    /// if the id does not exist.
    pub fn update(&self, id: i64, title: &str, body: &str) -> Result<()> {
        let rows_changed = self.conn.execute(
            "UPDATE local_drafts SET title = ?1, body = ?2 WHERE id = ?3",
            params![title, body, id],
        )?;
        if rows_changed == 0 {
            Err(LocalStoreError::NotFound(format!("draft id={id}")))
        } else {
            Ok(())
        }
    }

    /// Delete a draft by id.  Returns `Err(NotFound)` if absent.
    pub fn delete(&self, id: i64) -> Result<()> {
        let rows_changed = self
            .conn
            .execute("DELETE FROM local_drafts WHERE id = ?1", params![id])?;
        if rows_changed == 0 {
            Err(LocalStoreError::NotFound(format!("draft id={id}")))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::LocalDb;

    fn open() -> LocalDb {
        LocalDb::open_in_memory().unwrap()
    }

    #[test]
    fn save_and_find_roundtrip() {
        let db = open();
        let repo = DraftRepository::new(db.conn());
        let id = repo.save("Hello", "World").unwrap();
        let draft = repo.find(id).unwrap();
        assert_eq!(draft.title, "Hello");
        assert_eq!(draft.body, "World");
    }

    #[test]
    fn find_missing_draft_returns_not_found() {
        let db = open();
        let repo = DraftRepository::new(db.conn());
        match repo.find(999) {
            Err(LocalStoreError::NotFound(_)) => {}
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn list_returns_drafts_in_insert_order() {
        let db = open();
        let repo = DraftRepository::new(db.conn());
        repo.save("A", "body a").unwrap();
        repo.save("B", "body b").unwrap();
        let drafts = repo.list().unwrap();
        assert_eq!(drafts.len(), 2);
        assert_eq!(drafts[0].title, "A");
        assert_eq!(drafts[1].title, "B");
    }

    #[test]
    fn update_changes_title_and_body() {
        let db = open();
        let repo = DraftRepository::new(db.conn());
        let id = repo.save("Old title", "Old body").unwrap();
        repo.update(id, "New title", "New body").unwrap();
        let draft = repo.find(id).unwrap();
        assert_eq!(draft.title, "New title");
        assert_eq!(draft.body, "New body");
    }

    #[test]
    fn delete_removes_draft() {
        let db = open();
        let repo = DraftRepository::new(db.conn());
        let id = repo.save("to delete", "").unwrap();
        repo.delete(id).unwrap();
        match repo.find(id) {
            Err(LocalStoreError::NotFound(_)) => {}
            other => panic!("expected NotFound after delete, got {other:?}"),
        }
    }

    #[test]
    fn update_missing_draft_returns_not_found() {
        let db = open();
        let repo = DraftRepository::new(db.conn());
        match repo.update(999, "x", "y") {
            Err(LocalStoreError::NotFound(_)) => {}
            other => panic!("expected NotFound, got {other:?}"),
        }
    }
}
