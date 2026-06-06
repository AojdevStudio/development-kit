//! Behavior: the Notes sample product's local SQLite state (issue #37).
//!
//! Proves the FREE local product action works end-to-end through the seam's
//! [`LocalModule`](local_store::product_module::LocalModule): the Notes migration
//! applies additively on top of the baseline schema, a note created locally
//! persists, and listing returns it. Local SQLite is authoritative for this local
//! product work (ADR-0001) — never for billing. Exercised against an in-memory DB,
//! through the real `apply_module` / `create_note` / `list_notes` surface.

use rusqlite::Connection;

use local_store::product_module::apply_module;
use local_store::products::notes::{create_note, list_notes, Note, NotesLocal};

fn memory() -> Connection {
    Connection::open_in_memory().expect("in-memory DB")
}

/// A note created locally persists and reads back — the free local action.
#[test]
fn a_locally_created_note_persists_and_lists() {
    let conn = memory();
    apply_module(&conn, &NotesLocal).expect("notes module applies");

    let note = Note {
        id: "note_1".into(),
        title: "Spine".into(),
        body: "Notes proves the seam.".into(),
    };
    create_note(&conn, &note).expect("note is created locally");

    let listed = list_notes(&conn).expect("notes list");
    assert_eq!(
        listed,
        vec![note],
        "the created note reads back from local state"
    );
}

/// Applying the Notes module twice is idempotent — its migration runs once, the
/// same contract the baseline schema uses, so a product's schema is as safe to
/// re-apply as the spine's.
#[test]
fn applying_notes_twice_is_idempotent() {
    let conn = memory();
    apply_module(&conn, &NotesLocal).expect("first apply");
    let second = apply_module(&conn, &NotesLocal).expect("second apply");
    assert_eq!(second, 0, "the second apply runs no migrations");
}

/// The free list action returns nothing on a fresh store — an empty local state
/// is a valid, non-error state.
#[test]
fn listing_notes_on_a_fresh_store_is_empty() {
    let conn = memory();
    apply_module(&conn, &NotesLocal).expect("notes module applies");
    assert!(list_notes(&conn).expect("notes list").is_empty());
}
