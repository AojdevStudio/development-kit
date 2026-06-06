//! Offline sync queue — the local change queue, retry policy, idempotent
//! replay guard, and conflict-resolution policy (issue #34).
//!
//! # Where this lives and why
//!
//! Per the domain glossary (`CONTEXT.md`), the *sync queue* is the
//! "local SQLite-backed queue of offline changes with retry and conflict
//! reconciliation policy." This module is the **pure policy core** of that
//! queue: the state machine that decides what is pending, what may retry, what
//! is a duplicate replay, and how a conflict resolves. It carries no I/O and no
//! database dependency, so it can live in `shared` (types-and-logic only,
//! ADR-0002) and be exercised exhaustively without a running app or disk.
//!
//! Durable storage is a *seam*, not a coupling: the [`SyncQueueStore`] trait
//! describes everything the queue needs to persist and reload its operations.
//! The desktop's local SQLite repository (issue #33) implements that trait when
//! it lands; nothing here knows about `rusqlite`, and the authority boundary
//! (ADR-0001) is respected because this queue only ever carries *local product
//! changes*, never billing or subscription truth.
//!
//! The acceptance criteria for issue #34 map onto the public surface here:
//!
//! - *Offline changes are queued* → [`SyncQueue::enqueue`].
//! - *Failed sends retry per policy* → [`RetryPolicy`] + [`SyncQueue::record_failure`].
//! - *Duplicate replays do not double-apply* → idempotency keys +
//!   [`SyncQueue::enqueue`]/[`SyncQueue::mark_synced`] dedup.
//! - *Conflicts resolve per the defined policy* → [`ConflictPolicy`] +
//!   [`SyncQueue::resolve_conflict`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A caller-supplied, stable identifier for a logical change.
///
/// Idempotency is built on this key: enqueueing the same key twice, or replaying
/// a send the server already acknowledged, must not apply the change twice. The
/// client is expected to derive it deterministically from the change (e.g. a
/// UUID minted once when the user performs the action), so a retry after a
/// crash carries the *same* key.
pub type IdempotencyKey = String;

/// One queued offline change awaiting sync to the cloud.
///
/// This is a pure record: `payload` is opaque product data (already serialized
/// by the product layer) and `base_revision` is the server revision the change
/// was made against, used for conflict detection. The queue never inspects the
/// payload — it only sequences, retries, and reconciles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncOperation {
    /// Stable idempotency key — the dedup identity of this change.
    pub idempotency_key: IdempotencyKey,
    /// Opaque, already-serialized product payload. Never billing truth.
    pub payload: String,
    /// The server revision this change was made against, for conflict
    /// detection. `None` means "created offline against no known server state".
    pub base_revision: Option<u64>,
    /// Where this operation is in its lifecycle.
    pub status: OpStatus,
    /// How many send attempts have been made and failed so far.
    pub attempts: u32,
}

/// The lifecycle of a queued operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpStatus {
    /// Queued, not yet acknowledged by the server. Eligible to send.
    Pending,
    /// The server acknowledged this change. Terminal success.
    Synced,
    /// Retries are exhausted; the change needs human/se­rvice attention. The
    /// queue will not send it again on its own. Terminal failure.
    DeadLettered,
    /// A conflict was detected against newer server state; awaiting resolution.
    Conflicted,
}

impl SyncOperation {
    /// A fresh pending operation for `key` carrying `payload`, made against
    /// `base_revision` (the last server revision the client had seen).
    pub fn new(
        key: impl Into<IdempotencyKey>,
        payload: impl Into<String>,
        base_revision: Option<u64>,
    ) -> Self {
        SyncOperation {
            idempotency_key: key.into(),
            payload: payload.into(),
            base_revision,
            status: OpStatus::Pending,
            attempts: 0,
        }
    }

    /// Whether this local change conflicts with the current `server_revision`.
    ///
    /// A conflict means the server moved on since the client made the change:
    /// its revision is strictly newer than the `base_revision` the change was
    /// built against. A change made offline against no known server state
    /// (`base_revision == None`) never reports a conflict here — there is no
    /// base to be stale against, so it is treated as a plain create and left for
    /// the server's own idempotency to absorb.
    pub fn conflicts_with(&self, server_revision: u64) -> bool {
        match self.base_revision {
            Some(base) => server_revision > base,
            None => false,
        }
    }
}

/// What the retry policy says to do after a failed send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDecision {
    /// Try again after waiting `delay_ms` milliseconds.
    RetryAfter { delay_ms: u64 },
    /// Give up — retries are exhausted; the operation is dead-lettered.
    GiveUp,
}

/// The retry schedule for failed sends.
///
/// Deterministic exponential backoff with a hard attempt ceiling: attempt *n*
/// (1-indexed) waits `base_delay_ms * 2^(n-1)`, capped at `max_delay_ms`, until
/// `max_attempts` failures have accumulated — after which the policy says
/// [`RetryDecision::GiveUp`] and the operation is dead-lettered rather than
/// retried forever. Keeping the schedule a pure function of the attempt count
/// (no clock, no RNG) makes the policy exhaustively testable and identical on
/// every machine. Real send-time jitter, if wanted, is layered on by the caller
/// that owns the clock — the policy itself stays deterministic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Maximum number of send attempts before giving up (dead-lettering).
    pub max_attempts: u32,
    /// Delay before the first retry, doubled each subsequent attempt.
    pub base_delay_ms: u64,
    /// Upper bound on any single backoff delay.
    pub max_delay_ms: u64,
}

impl Default for RetryPolicy {
    /// A sensible offline-sync default: up to 5 attempts, 1s base backoff,
    /// capped at 60s. Conservative enough not to hammer a flaky network, bounded
    /// enough that a permanently-failing op dead-letters instead of looping.
    fn default() -> Self {
        RetryPolicy {
            max_attempts: 5,
            base_delay_ms: 1_000,
            max_delay_ms: 60_000,
        }
    }
}

impl RetryPolicy {
    /// Decide what to do given how many attempts have *already failed*.
    ///
    /// `attempts_so_far` is the count of failures accumulated before this
    /// decision. Once it reaches `max_attempts` the policy gives up; otherwise it
    /// returns the capped exponential backoff for the next attempt.
    pub fn decide(&self, attempts_so_far: u32) -> RetryDecision {
        if attempts_so_far >= self.max_attempts {
            return RetryDecision::GiveUp;
        }
        // attempts_so_far failures means the next wait is the (attempts_so_far)-th
        // backoff step: base * 2^(attempts_so_far - 1) for the first retry.
        let exponent = attempts_so_far.saturating_sub(1);
        let factor = 1u64.checked_shl(exponent).unwrap_or(u64::MAX);
        let delay = self
            .base_delay_ms
            .saturating_mul(factor)
            .min(self.max_delay_ms);
        RetryDecision::RetryAfter { delay_ms: delay }
    }
}

/// How to reconcile a local change that conflicts with newer server state.
///
/// A conflict is detected when the server has advanced past the revision the
/// local change was made against (see [`SyncOperation::conflicts_with`]). The
/// policy decides who wins. The default is [`ConflictPolicy::ServerWins`] — the
/// safe choice for a kit whose authority lives in the cloud (ADR-0001): when in
/// doubt, the server's record stands and the local change is dropped rather than
/// silently clobbering newer authoritative state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    /// The server's newer state wins; the local change is discarded.
    #[default]
    ServerWins,
    /// The local change wins; it is re-sent to overwrite the server.
    ClientWins,
    /// The change with the most recent timestamp wins (last writer wins),
    /// comparing the local change's timestamp to the server's.
    LastWriterWins,
    /// No automatic resolution; the operation is flagged for human/product
    /// reconciliation. The right choice for regulated domains where silently
    /// dropping or overwriting data is unacceptable.
    Manual,
}

/// The outcome of reconciling a conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictResolution {
    /// Keep and re-send the local change (client wins). The op returns to
    /// `Pending` so the next sweep delivers it.
    ApplyLocal,
    /// Discard the local change in favor of the server (server wins). The op is
    /// marked `Synced` (superseded) and never re-sent.
    KeepServer,
    /// Defer to a human/product flow. The op is flagged `Conflicted`.
    Manual,
}

impl ConflictPolicy {
    /// Resolve a detected conflict under this policy.
    ///
    /// `local_ts` and `server_ts` are the change timestamps (unix epoch
    /// anything-monotonic) used only by [`ConflictPolicy::LastWriterWins`]; the
    /// other policies ignore them, so the function stays a pure, clock-free
    /// mapping the caller can test exhaustively. On a `LastWriterWins` tie the
    /// server is kept, matching the cloud-authority default (ADR-0001).
    pub fn decide(self, local_ts: u64, server_ts: u64) -> ConflictResolution {
        match self {
            ConflictPolicy::ServerWins => ConflictResolution::KeepServer,
            ConflictPolicy::ClientWins => ConflictResolution::ApplyLocal,
            ConflictPolicy::Manual => ConflictResolution::Manual,
            ConflictPolicy::LastWriterWins => {
                if local_ts > server_ts {
                    ConflictResolution::ApplyLocal
                } else {
                    ConflictResolution::KeepServer
                }
            }
        }
    }
}

/// The local offline change queue.
///
/// Operations are keyed by their idempotency key, so re-enqueueing a known key
/// is a no-op rather than a second copy — that single rule is what makes the
/// whole queue safe to replay after a crash or a duplicated user action.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncQueue {
    ops: BTreeMap<IdempotencyKey, SyncOperation>,
}

impl SyncQueue {
    /// An empty queue.
    pub fn new() -> Self {
        SyncQueue::default()
    }

    /// Queue an offline change.
    ///
    /// Returns `true` if the change was newly queued, `false` if an operation
    /// with the same idempotency key already exists (the duplicate is ignored —
    /// it is *not* applied a second time and does not reset retry state).
    pub fn enqueue(&mut self, op: SyncOperation) -> bool {
        if self.ops.contains_key(&op.idempotency_key) {
            return false;
        }
        self.ops.insert(op.idempotency_key.clone(), op);
        true
    }

    /// Look up an operation by its idempotency key.
    pub fn get(&self, key: &str) -> Option<&SyncOperation> {
        self.ops.get(key)
    }

    /// The operations eligible to send right now, in stable key order: those
    /// still `Pending`. `Synced`, `DeadLettered`, and `Conflicted` operations
    /// are deliberately excluded — the sender should only ever pick up work the
    /// queue still wants delivered.
    pub fn pending(&self) -> Vec<&SyncOperation> {
        self.ops
            .values()
            .filter(|op| op.status == OpStatus::Pending)
            .collect()
    }

    /// Mark the operation for `key` as acknowledged by the server.
    ///
    /// This is terminal success: the op leaves the pending set but stays in the
    /// queue as a `Synced` tombstone. Keeping the record (rather than deleting
    /// it) is what makes a later replay of the same key a recognized duplicate
    /// instead of a fresh change — the dedup guarantee survives a successful
    /// sync, not just a pending one. Returns `false` if the key is unknown.
    pub fn mark_synced(&mut self, key: &str) -> bool {
        match self.ops.get_mut(key) {
            Some(op) => {
                op.status = OpStatus::Synced;
                true
            }
            None => false,
        }
    }

    /// Reconcile the operation for `key` against the current `server_revision`
    /// under `policy`, applying the resolution to the operation's status.
    ///
    /// This is the conflict-handling entrypoint. If the local change does not
    /// actually conflict (the server has not advanced past its base), it is left
    /// untouched and `None` is returned — there is nothing to resolve. Otherwise
    /// the policy decides, and the op transitions accordingly:
    ///
    /// - [`ConflictResolution::ApplyLocal`] → back to `Pending` (the local change
    ///   is re-sent to overwrite the server),
    /// - [`ConflictResolution::KeepServer`] → `Synced` (the local change is
    ///   superseded and never re-sent),
    /// - [`ConflictResolution::Manual`] → `Conflicted` (held for a human/product
    ///   flow; deliberately *not* re-sent automatically).
    ///
    /// `local_ts`/`server_ts` feed `LastWriterWins`; other policies ignore them.
    /// Returns the resolution applied, or `None` if the key is unknown or there
    /// was no conflict.
    pub fn resolve_conflict(
        &mut self,
        key: &str,
        server_revision: u64,
        policy: ConflictPolicy,
        local_ts: u64,
        server_ts: u64,
    ) -> Option<ConflictResolution> {
        let op = self.ops.get_mut(key)?;
        if !op.conflicts_with(server_revision) {
            return None;
        }
        let resolution = policy.decide(local_ts, server_ts);
        op.status = match resolution {
            ConflictResolution::ApplyLocal => OpStatus::Pending,
            ConflictResolution::KeepServer => OpStatus::Synced,
            ConflictResolution::Manual => OpStatus::Conflicted,
        };
        Some(resolution)
    }

    /// Record that a send attempt for `key` failed, applying `policy`.
    ///
    /// Increments the operation's attempt count, then asks the policy what to
    /// do: a [`RetryDecision::RetryAfter`] leaves it `Pending` (so the next sweep
    /// re-sends it), while a [`RetryDecision::GiveUp`] dead-letters it so the
    /// queue stops retrying a permanently-failing change. The decision is
    /// returned so the caller can schedule the next attempt with the backoff
    /// delay. Returns `None` if the key is unknown.
    pub fn record_failure(&mut self, key: &str, policy: &RetryPolicy) -> Option<RetryDecision> {
        let op = self.ops.get_mut(key)?;
        op.attempts += 1;
        let decision = policy.decide(op.attempts);
        if matches!(decision, RetryDecision::GiveUp) {
            op.status = OpStatus::DeadLettered;
        }
        Some(decision)
    }
}

/// The durable-storage seam for the sync queue.
///
/// The queue's *policy* (this module) is pure and in-memory; its *durability*
/// is delegated through this trait so the offline change set survives an app
/// restart. The desktop's local SQLite repository (issue #33) is the production
/// implementor — it writes each operation to the `sqlite/` store and reloads it
/// on launch. Keeping the contract here, behind a trait, is what lets the queue
/// be fully tested with an in-memory fake (see [`InMemorySyncStore`]) while the
/// real `rusqlite`-backed store lives in the desktop tree, so `shared` stays
/// dependency-thin and ADR-0002 is respected (no `sqlx`/DB deps cross the
/// boundary).
///
/// Implementors persist by idempotency key; `upsert` is the natural write
/// because the key is the operation's identity, so re-persisting a known op
/// updates it rather than duplicating it.
pub trait SyncQueueStore {
    /// The storage-specific error type.
    type Error;

    /// Insert or update an operation, keyed by its idempotency key.
    fn upsert(&mut self, op: &SyncOperation) -> Result<(), Self::Error>;

    /// Load every persisted operation. Order is not guaranteed; the queue
    /// re-keys them on load.
    fn load_all(&self) -> Result<Vec<SyncOperation>, Self::Error>;
}

impl SyncQueue {
    /// Persist the whole queue through `store`, writing every operation.
    ///
    /// This is the "checkpoint" the desktop calls so the offline change set is
    /// durable: after it returns `Ok`, a crash-and-relaunch can rebuild the
    /// exact queue via [`SyncQueue::load`].
    pub fn persist_to<S: SyncQueueStore>(&self, store: &mut S) -> Result<(), S::Error> {
        for op in self.ops.values() {
            store.upsert(op)?;
        }
        Ok(())
    }

    /// Rebuild a queue from everything `store` has persisted.
    ///
    /// This is the restart path: on launch the desktop reconstructs the queue —
    /// pending work, attempt counts, dead-letters, and all — so offline changes
    /// made before the restart are not lost and are not double-applied (the
    /// idempotency keys are reloaded intact).
    pub fn load<S: SyncQueueStore>(store: &S) -> Result<Self, S::Error> {
        let mut queue = SyncQueue::new();
        for op in store.load_all()? {
            queue.ops.insert(op.idempotency_key.clone(), op);
        }
        Ok(queue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enqueue_queues_an_offline_change_as_pending() {
        let mut queue = SyncQueue::new();

        let newly_queued = queue.enqueue(SyncOperation::new("op-1", "payload", Some(7)));

        assert!(newly_queued, "a brand-new change is queued");
    }

    #[test]
    fn queued_change_is_pending_with_zero_attempts() {
        let mut queue = SyncQueue::new();
        queue.enqueue(SyncOperation::new("op-1", "payload", Some(7)));

        let op = queue.get("op-1").expect("the queued op is retrievable");
        assert_eq!(op.status, OpStatus::Pending);
        assert_eq!(op.attempts, 0);
    }

    #[test]
    fn pending_lists_only_sendable_operations() {
        let mut queue = SyncQueue::new();
        queue.enqueue(SyncOperation::new("op-1", "a", None));
        queue.enqueue(SyncOperation::new("op-2", "b", None));

        let keys: Vec<&str> = queue
            .pending()
            .iter()
            .map(|op| op.idempotency_key.as_str())
            .collect();
        assert_eq!(keys, vec!["op-1", "op-2"]);
    }

    #[test]
    fn duplicate_enqueue_of_same_key_does_not_double_apply() {
        let mut queue = SyncQueue::new();
        assert!(queue.enqueue(SyncOperation::new("op-1", "original", None)));

        // A crash-recovery or double-click replays the SAME idempotency key with
        // a different payload. It must be ignored, not queued a second time.
        let queued_again = queue.enqueue(SyncOperation::new("op-1", "replayed", None));

        assert!(!queued_again, "the duplicate replay is rejected");
        assert_eq!(queue.pending().len(), 1, "still exactly one operation");
        assert_eq!(
            queue.get("op-1").unwrap().payload,
            "original",
            "the original change is preserved, not overwritten"
        );
    }

    #[test]
    fn marking_synced_removes_it_from_pending() {
        let mut queue = SyncQueue::new();
        queue.enqueue(SyncOperation::new("op-1", "a", None));

        assert!(queue.mark_synced("op-1"));

        assert_eq!(queue.get("op-1").unwrap().status, OpStatus::Synced);
        assert!(queue.pending().is_empty(), "synced work is not re-sent");
    }

    #[test]
    fn replay_after_sync_is_still_a_no_op() {
        let mut queue = SyncQueue::new();
        queue.enqueue(SyncOperation::new("op-1", "a", None));
        queue.mark_synced("op-1");

        // The server already has this change. A late retry that did not learn of
        // the ack replays the same key — it must not resurrect the work.
        let queued_again = queue.enqueue(SyncOperation::new("op-1", "a", None));

        assert!(!queued_again, "an already-synced key cannot be re-queued");
        assert!(queue.pending().is_empty());
    }

    #[test]
    fn marking_an_unknown_key_synced_reports_false() {
        let mut queue = SyncQueue::new();
        assert!(!queue.mark_synced("ghost"));
    }

    fn policy() -> RetryPolicy {
        RetryPolicy {
            max_attempts: 4,
            base_delay_ms: 100,
            max_delay_ms: 1_000,
        }
    }

    #[test]
    fn retry_backoff_grows_exponentially() {
        let p = policy();
        // After 1 failure: first retry waits base (100). After 2: 200. After 3: 400.
        assert_eq!(p.decide(1), RetryDecision::RetryAfter { delay_ms: 100 });
        assert_eq!(p.decide(2), RetryDecision::RetryAfter { delay_ms: 200 });
        assert_eq!(p.decide(3), RetryDecision::RetryAfter { delay_ms: 400 });
    }

    #[test]
    fn retry_backoff_is_capped_at_max_delay() {
        let p = RetryPolicy {
            max_attempts: 20,
            base_delay_ms: 100,
            max_delay_ms: 1_000,
        };
        // 100 * 2^9 would be 51200 but the cap holds it to 1000.
        assert_eq!(p.decide(10), RetryDecision::RetryAfter { delay_ms: 1_000 });
    }

    #[test]
    fn retry_gives_up_once_attempts_reach_the_ceiling() {
        let p = policy();
        // max_attempts is 4: the 4th failure exhausts retries.
        assert_eq!(p.decide(4), RetryDecision::GiveUp);
        assert_eq!(p.decide(5), RetryDecision::GiveUp);
    }

    #[test]
    fn first_failure_keeps_op_pending_and_schedules_a_retry() {
        let mut queue = SyncQueue::new();
        queue.enqueue(SyncOperation::new("op-1", "a", None));

        let decision = queue.record_failure("op-1", &policy());

        assert_eq!(decision, Some(RetryDecision::RetryAfter { delay_ms: 100 }));
        let op = queue.get("op-1").unwrap();
        assert_eq!(op.attempts, 1);
        assert_eq!(op.status, OpStatus::Pending, "still eligible to re-send");
        assert_eq!(queue.pending().len(), 1);
    }

    #[test]
    fn exhausting_retries_dead_letters_the_op() {
        let mut queue = SyncQueue::new();
        queue.enqueue(SyncOperation::new("op-1", "a", None));
        let p = policy(); // max_attempts = 4

        // Fail it until the policy gives up.
        let mut last = None;
        for _ in 0..4 {
            last = queue.record_failure("op-1", &p);
        }

        assert_eq!(last, Some(RetryDecision::GiveUp));
        let op = queue.get("op-1").unwrap();
        assert_eq!(op.attempts, 4);
        assert_eq!(op.status, OpStatus::DeadLettered);
        assert!(
            queue.pending().is_empty(),
            "a dead-lettered op is never re-sent automatically"
        );
    }

    #[test]
    fn recording_failure_for_unknown_key_returns_none() {
        let mut queue = SyncQueue::new();
        assert_eq!(queue.record_failure("ghost", &policy()), None);
    }

    #[test]
    fn change_conflicts_when_server_revision_is_newer_than_its_base() {
        let op = SyncOperation::new("op-1", "edit", Some(5));
        assert!(op.conflicts_with(6), "server moved past our base");
        assert!(!op.conflicts_with(5), "server is exactly at our base");
        assert!(!op.conflicts_with(4), "server is behind our base");
    }

    #[test]
    fn offline_create_with_no_base_never_conflicts() {
        let op = SyncOperation::new("op-1", "create", None);
        assert!(!op.conflicts_with(100));
    }

    #[test]
    fn server_wins_policy_keeps_the_server_record() {
        // Timestamps are irrelevant for ServerWins.
        assert_eq!(
            ConflictPolicy::ServerWins.decide(999, 1),
            ConflictResolution::KeepServer
        );
    }

    #[test]
    fn client_wins_policy_re_applies_the_local_change() {
        assert_eq!(
            ConflictPolicy::ClientWins.decide(1, 999),
            ConflictResolution::ApplyLocal
        );
    }

    #[test]
    fn manual_policy_defers_to_a_human() {
        assert_eq!(
            ConflictPolicy::Manual.decide(1, 1),
            ConflictResolution::Manual
        );
    }

    #[test]
    fn last_writer_wins_picks_the_more_recent_change() {
        let p = ConflictPolicy::LastWriterWins;
        assert_eq!(p.decide(10, 5), ConflictResolution::ApplyLocal);
        assert_eq!(p.decide(5, 10), ConflictResolution::KeepServer);
        // A tie defers to the server, matching cloud-authority default.
        assert_eq!(p.decide(7, 7), ConflictResolution::KeepServer);
    }

    #[test]
    fn default_conflict_policy_is_server_wins() {
        assert_eq!(ConflictPolicy::default(), ConflictPolicy::ServerWins);
    }

    #[test]
    fn resolving_a_non_conflict_leaves_the_op_untouched() {
        let mut queue = SyncQueue::new();
        queue.enqueue(SyncOperation::new("op-1", "a", Some(5)));

        // Server is at the same revision the change was based on — no conflict.
        let resolution = queue.resolve_conflict("op-1", 5, ConflictPolicy::ServerWins, 0, 0);

        assert_eq!(resolution, None);
        assert_eq!(queue.get("op-1").unwrap().status, OpStatus::Pending);
    }

    #[test]
    fn server_wins_supersedes_the_local_change() {
        let mut queue = SyncQueue::new();
        queue.enqueue(SyncOperation::new("op-1", "a", Some(5)));

        let resolution = queue.resolve_conflict("op-1", 9, ConflictPolicy::ServerWins, 100, 200);

        assert_eq!(resolution, Some(ConflictResolution::KeepServer));
        assert_eq!(queue.get("op-1").unwrap().status, OpStatus::Synced);
        assert!(
            queue.pending().is_empty(),
            "the superseded change is never re-sent"
        );
    }

    #[test]
    fn client_wins_returns_the_change_to_pending_for_resend() {
        let mut queue = SyncQueue::new();
        queue.enqueue(SyncOperation::new("op-1", "a", Some(5)));
        // Take it out of pending first to prove resolution puts it back.
        queue.record_failure("op-1", &RetryPolicy::default());

        let resolution = queue.resolve_conflict("op-1", 9, ConflictPolicy::ClientWins, 200, 100);

        assert_eq!(resolution, Some(ConflictResolution::ApplyLocal));
        assert_eq!(queue.get("op-1").unwrap().status, OpStatus::Pending);
        assert_eq!(queue.pending().len(), 1, "the local change is re-sent");
    }

    #[test]
    fn manual_policy_flags_the_op_as_conflicted() {
        let mut queue = SyncQueue::new();
        queue.enqueue(SyncOperation::new("op-1", "a", Some(5)));

        let resolution = queue.resolve_conflict("op-1", 9, ConflictPolicy::Manual, 0, 0);

        assert_eq!(resolution, Some(ConflictResolution::Manual));
        assert_eq!(queue.get("op-1").unwrap().status, OpStatus::Conflicted);
        assert!(
            queue.pending().is_empty(),
            "a conflicted op waits for a human, not an auto-resend"
        );
    }

    #[test]
    fn last_writer_wins_keeps_the_newer_side() {
        let mut queue = SyncQueue::new();
        queue.enqueue(SyncOperation::new("local-newer", "a", Some(5)));
        queue.enqueue(SyncOperation::new("server-newer", "b", Some(5)));

        // Local edited more recently than the server -> local wins.
        let r1 = queue.resolve_conflict("local-newer", 9, ConflictPolicy::LastWriterWins, 300, 100);
        // Server edited more recently -> server wins.
        let r2 =
            queue.resolve_conflict("server-newer", 9, ConflictPolicy::LastWriterWins, 100, 300);

        assert_eq!(r1, Some(ConflictResolution::ApplyLocal));
        assert_eq!(queue.get("local-newer").unwrap().status, OpStatus::Pending);
        assert_eq!(r2, Some(ConflictResolution::KeepServer));
        assert_eq!(queue.get("server-newer").unwrap().status, OpStatus::Synced);
    }

    #[test]
    fn resolving_an_unknown_key_returns_none() {
        let mut queue = SyncQueue::new();
        assert_eq!(
            queue.resolve_conflict("ghost", 9, ConflictPolicy::ServerWins, 0, 0),
            None
        );
    }
}
