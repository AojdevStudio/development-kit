//! Integration test for the offline sync queue's durability seam (issue #34).
//!
//! The offline queue's central promise is that work done offline is not lost:
//! it must survive an app restart. This test exercises that promise through the
//! public [`SyncQueueStore`] seam using an in-memory fake that stands in for the
//! desktop's real SQLite store (issue #33). "Restart" is modeled honestly: the
//! in-memory `SyncQueue` is dropped entirely and a fresh one is rebuilt *only*
//! from what was persisted — exactly what `rusqlite`-backed reload does on
//! launch. Because the policy core carries no I/O, this seam is all that the
//! durable store has to implement, and it is verified here without a database.

use std::collections::HashMap;
use std::convert::Infallible;

use shared::{OpStatus, RetryPolicy, SyncOperation, SyncQueue, SyncQueueStore};

/// A durable store backed by a `HashMap`, standing in for the desktop's SQLite
/// repository. It persists by idempotency key (the operation's identity), so an
/// upsert of a known op overwrites rather than duplicating — the same contract
/// the SQLite `INSERT ... ON CONFLICT(idempotency_key) DO UPDATE` will honor.
#[derive(Default)]
struct InMemorySyncStore {
    rows: HashMap<String, SyncOperation>,
}

impl SyncQueueStore for InMemorySyncStore {
    type Error = Infallible;

    fn upsert(&mut self, op: &SyncOperation) -> Result<(), Self::Error> {
        self.rows.insert(op.idempotency_key.clone(), op.clone());
        Ok(())
    }

    fn load_all(&self) -> Result<Vec<SyncOperation>, Self::Error> {
        Ok(self.rows.values().cloned().collect())
    }
}

#[test]
fn queued_offline_changes_survive_a_restart() {
    let mut store = InMemorySyncStore::default();

    // --- before the "restart": build a queue with mixed operation states ---
    {
        let mut queue = SyncQueue::new();
        queue.enqueue(SyncOperation::new("op-pending", "draft-edit", Some(3)));
        queue.enqueue(SyncOperation::new(
            "op-failed-once",
            "another-edit",
            Some(3),
        ));
        // Accumulate one failed attempt so we can prove retry state is durable.
        queue.record_failure("op-failed-once", &RetryPolicy::default());

        queue
            .persist_to(&mut store)
            .expect("persist is infallible here");
        // The in-memory queue is dropped at the end of this scope — only the
        // store survives, exactly as in a real process exit.
    }

    // --- after the "restart": rebuild purely from durable storage ---
    let reloaded = SyncQueue::load(&store).expect("reload is infallible here");

    // Both offline changes came back.
    assert_eq!(
        reloaded.pending().len(),
        2,
        "both pending changes survived the restart"
    );

    // The fresh change is intact.
    let pending = reloaded.get("op-pending").expect("op-pending reloaded");
    assert_eq!(pending.status, OpStatus::Pending);
    assert_eq!(pending.attempts, 0);
    assert_eq!(pending.payload, "draft-edit");
    assert_eq!(pending.base_revision, Some(3));

    // Retry progress is durable: the failed-once op kept its attempt count, so
    // the retry budget is not silently reset by a restart.
    let failed = reloaded
        .get("op-failed-once")
        .expect("op-failed-once reloaded");
    assert_eq!(failed.attempts, 1, "retry state survived the restart");
}

#[test]
fn replaying_a_persisted_change_after_restart_does_not_double_apply() {
    let mut store = InMemorySyncStore::default();

    {
        let mut queue = SyncQueue::new();
        queue.enqueue(SyncOperation::new("op-1", "the-only-change", Some(1)));
        queue.persist_to(&mut store).unwrap();
    }

    // On relaunch the queue is rebuilt from storage...
    let mut reloaded = SyncQueue::load(&store).unwrap();

    // ...and a recovery path re-enqueues the same idempotency key (e.g. the UI
    // replays an action it is unsure completed). The reload preserved the key,
    // so the duplicate is recognized and ignored — the change is not queued, or
    // applied, twice.
    let queued_again = reloaded.enqueue(SyncOperation::new("op-1", "the-only-change", Some(1)));

    assert!(
        !queued_again,
        "the persisted key is recognized as a duplicate"
    );
    assert_eq!(reloaded.pending().len(), 1, "still exactly one change");
}
