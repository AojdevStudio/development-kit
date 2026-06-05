//! Cloud audit/event service — the authority recorder (ADR-0001).
//!
//! This module owns the `record()` interface: callers pass an [`AuditEvent`]
//! and the recorder writes it to the durable store. Per ADR-0001 the cloud
//! Postgres database is the authority for audit events; the backend issues
//! them, not the desktop client.
//!
//! The [`AuditStore`] trait decouples the recorder from the concrete Postgres
//! adapter, keeping the business logic testable in isolation. A real
//! `PgAuditStore` wired to `sqlx` will be introduced when the database
//! migration infrastructure (a later issue) is in place.

use shared::{AuditEvent, Sensitivity};

/// Errors that the audit service can return.
#[derive(Debug, PartialEq, Eq)]
pub enum AuditError {
    /// The underlying store rejected or failed to persist the event.
    StorageFailure(String),
}

impl std::fmt::Display for AuditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuditError::StorageFailure(msg) => write!(f, "audit storage failure: {msg}"),
        }
    }
}

/// The persistence contract for audit events.
///
/// Implementations can be a real Postgres adapter, an in-memory store for
/// tests, or a no-op for contexts that deliberately suppress recording.
pub trait AuditStore: Send + Sync {
    /// Persist `event`. Returns `Err` only if the store cannot accept the
    /// event and the caller should treat the action as failed.
    fn save(&self, event: &AuditEvent) -> Result<(), AuditError>;

    /// Return all stored events, in insertion order. Used by the query/review
    /// interface and tests; production implementations may paginate.
    fn list(&self) -> Vec<AuditEvent>;
}

/// The audit service. Holds a store reference and enforces that sensitive
/// events are always persisted (callers cannot skip them).
pub struct AuditService<S: AuditStore> {
    store: S,
}

impl<S: AuditStore> AuditService<S> {
    pub fn new(store: S) -> Self {
        AuditService { store }
    }

    /// Record an audit event.
    ///
    /// Sensitive events (`Sensitivity::Sensitive`) are always forwarded to the
    /// store, even if the store itself is in a degraded state — callers that
    /// care about guaranteed delivery should handle the returned `Err`.
    pub fn record(&self, event: &AuditEvent) -> Result<(), AuditError> {
        self.store.save(event)
    }

    /// Return all recorded events. Callers use this to present a reviewable
    /// audit trail to operators.
    pub fn query_all(&self) -> Vec<AuditEvent> {
        self.store.list()
    }

    /// Return only the events matching `action`. Convenience for drilling into
    /// a single action type without loading the full log.
    pub fn query_by_action(&self, action: &str) -> Vec<AuditEvent> {
        self.store
            .list()
            .into_iter()
            .filter(|e| e.action == action)
            .collect()
    }

    /// Return only events tagged as sensitive. Finance / healthcare operators
    /// often want to review this slice in isolation.
    pub fn query_sensitive(&self) -> Vec<AuditEvent> {
        self.store
            .list()
            .into_iter()
            .filter(|e| e.sensitivity == Sensitivity::Sensitive)
            .collect()
    }
}

// ---------------------------------------------------------------------------
// In-memory store — for tests and local-dev mock mode only.
// ---------------------------------------------------------------------------

use std::sync::Mutex;

/// An in-memory [`AuditStore`] backed by a `Vec`. Thread-safe via `Mutex`.
/// Not suitable for production; used in tests and local mock mode.
pub struct InMemoryAuditStore {
    events: Mutex<Vec<AuditEvent>>,
}

impl InMemoryAuditStore {
    pub fn new() -> Self {
        InMemoryAuditStore {
            events: Mutex::new(Vec::new()),
        }
    }
}

impl Default for InMemoryAuditStore {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditStore for InMemoryAuditStore {
    fn save(&self, event: &AuditEvent) -> Result<(), AuditError> {
        self.events
            .lock()
            .expect("InMemoryAuditStore lock poisoned")
            .push(event.clone());
        Ok(())
    }

    fn list(&self) -> Vec<AuditEvent> {
        self.events
            .lock()
            .expect("InMemoryAuditStore lock poisoned")
            .clone()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use shared::{ActorKind, Sensitivity};

    // ---- helpers -----------------------------------------------------------

    fn make_event(id: &str, action: &str, sensitivity: Sensitivity) -> AuditEvent {
        AuditEvent {
            event_id: id.to_string(),
            occurred_at: "2026-06-05T12:00:00Z".to_string(),
            account_id: "acct_test".to_string(),
            user_id: Some("user_test".to_string()),
            actor_kind: ActorKind::User,
            action: action.to_string(),
            sensitivity,
            payload: serde_json::json!({ "detail": "test" }),
        }
    }

    fn service() -> AuditService<InMemoryAuditStore> {
        AuditService::new(InMemoryAuditStore::new())
    }

    // ---- Cycle 2a: record() persists an event ------------------------------

    #[test]
    fn record_persists_a_standard_event() {
        let svc = service();
        let ev = make_event("evt_001", "license.refreshed", Sensitivity::Standard);

        svc.record(&ev).expect("record should succeed");

        let all = svc.query_all();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].event_id, "evt_001");
    }

    // ---- Cycle 2b: multiple events preserved in insertion order ------------

    #[test]
    fn query_all_returns_events_in_insertion_order() {
        let svc = service();
        let e1 = make_event("evt_001", "license.refreshed", Sensitivity::Standard);
        let e2 = make_event(
            "evt_002",
            "billing.subscription_created",
            Sensitivity::Sensitive,
        );

        svc.record(&e1).unwrap();
        svc.record(&e2).unwrap();

        let all = svc.query_all();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].event_id, "evt_001");
        assert_eq!(all[1].event_id, "evt_002");
    }

    // ---- Cycle 2c: query_by_action filters correctly -----------------------

    #[test]
    fn query_by_action_returns_only_matching_events() {
        let svc = service();
        svc.record(&make_event(
            "e1",
            "license.refreshed",
            Sensitivity::Standard,
        ))
        .unwrap();
        svc.record(&make_event(
            "e2",
            "billing.subscription_created",
            Sensitivity::Sensitive,
        ))
        .unwrap();
        svc.record(&make_event(
            "e3",
            "license.refreshed",
            Sensitivity::Standard,
        ))
        .unwrap();

        let license_events = svc.query_by_action("license.refreshed");
        assert_eq!(license_events.len(), 2);
        assert!(license_events
            .iter()
            .all(|e| e.action == "license.refreshed"));
    }

    #[test]
    fn query_by_action_returns_empty_when_no_match() {
        let svc = service();
        svc.record(&make_event(
            "e1",
            "license.refreshed",
            Sensitivity::Standard,
        ))
        .unwrap();

        let result = svc.query_by_action("billing.subscription_created");
        assert!(result.is_empty());
    }

    // ---- Cycle 2d: sensitive events are queryable in isolation -------------

    #[test]
    fn query_sensitive_returns_only_sensitive_events() {
        let svc = service();
        svc.record(&make_event(
            "e1",
            "license.refreshed",
            Sensitivity::Standard,
        ))
        .unwrap();
        svc.record(&make_event(
            "e2",
            "billing.subscription_created",
            Sensitivity::Sensitive,
        ))
        .unwrap();
        svc.record(&make_event(
            "e3",
            "permission.role_changed",
            Sensitivity::Sensitive,
        ))
        .unwrap();

        let sensitive = svc.query_sensitive();
        assert_eq!(sensitive.len(), 2);
        assert!(sensitive.iter().all(|e| e.is_sensitive()));
    }

    #[test]
    fn query_sensitive_returns_empty_when_no_sensitive_events() {
        let svc = service();
        svc.record(&make_event(
            "e1",
            "license.refreshed",
            Sensitivity::Standard,
        ))
        .unwrap();

        assert!(svc.query_sensitive().is_empty());
    }

    // ---- Cycle 2e: system-actor event (no user_id) is recorded correctly --

    #[test]
    fn system_event_without_user_id_is_recorded() {
        let svc = service();
        let ev = AuditEvent {
            event_id: "evt_sys".to_string(),
            occurred_at: "2026-06-05T12:00:00Z".to_string(),
            account_id: "acct_test".to_string(),
            user_id: None,
            actor_kind: ActorKind::System,
            action: "billing.trial_expired".to_string(),
            sensitivity: Sensitivity::Sensitive,
            payload: serde_json::Value::Null,
        };

        svc.record(&ev).unwrap();
        let all = svc.query_all();
        assert_eq!(all.len(), 1);
        assert!(all[0].user_id.is_none());
        assert_eq!(all[0].actor_kind, ActorKind::System);
    }

    // ---- Cycle 2f: storage-error propagation --------------------------------

    #[test]
    fn storage_error_is_returned_to_caller() {
        struct FailingStore;
        impl AuditStore for FailingStore {
            fn save(&self, _: &AuditEvent) -> Result<(), AuditError> {
                Err(AuditError::StorageFailure("disk full".to_string()))
            }
            fn list(&self) -> Vec<AuditEvent> {
                vec![]
            }
        }

        let svc = AuditService::new(FailingStore);
        let ev = make_event("e1", "license.refreshed", Sensitivity::Standard);
        let err = svc.record(&ev).unwrap_err();
        assert_eq!(err, AuditError::StorageFailure("disk full".to_string()));
    }
}
