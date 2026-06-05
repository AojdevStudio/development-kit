//! Audit / event DTO — the stable shared contract for the audit/event service.
//!
//! Both the desktop (read-only local access) and the cloud backend write/read
//! this type. Per ADR-0001 the *durable* store is cloud Postgres; local SQLite
//! may cache a subset for display, but the backend is the authority.
//!
//! This crate is types-only (ADR-0002): no sqlx, no Stripe, no secret loaders.

use serde::{Deserialize, Serialize};

/// The category of the actor that triggered the event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    /// An authenticated end-user.
    User,
    /// An automated system or background job.
    System,
}

/// Classification of how sensitive an event is.
///
/// Sensitive events (e.g. billing changes, permission changes) must always
/// produce an audit entry even when less-sensitive events might be sampled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sensitivity {
    /// Routine informational event.
    Standard,
    /// Sensitive action (billing, permission, security-relevant).
    Sensitive,
}

/// A single structured audit event.
///
/// This is the stable wire/storage type shared across the desktop app and the
/// cloud backend. Fields are chosen to be queryable and reviewable for
/// healthcare and finance operators.
///
/// `action` is a dot-separated namespaced string such as `"license.refreshed"`
/// or `"billing.subscription_created"`. Callers must use stable strings —
/// treat them as part of the API contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Unique identifier for this event, assigned by the recorder.
    pub event_id: String,
    /// ISO-8601 UTC timestamp when the event occurred (seconds precision is sufficient).
    pub occurred_at: String,
    /// The account this event belongs to.
    pub account_id: String,
    /// Optional user identifier — absent for system-initiated events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    /// Who or what triggered the event.
    pub actor_kind: ActorKind,
    /// Stable dot-separated action name, e.g. `"license.refreshed"`.
    pub action: String,
    /// Sensitivity classification — callers are responsible for tagging
    /// security/billing actions as `Sensitive`.
    pub sensitivity: Sensitivity,
    /// Arbitrary structured payload. Callers should keep this small and
    /// redact PII before recording.
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub payload: serde_json::Value,
}

impl AuditEvent {
    /// Whether this event must never be dropped or sampled out.
    pub fn is_sensitive(&self) -> bool {
        self.sensitivity == Sensitivity::Sensitive
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- helpers -----------------------------------------------------------

    fn standard_event() -> AuditEvent {
        AuditEvent {
            event_id: "evt_001".into(),
            occurred_at: "2026-06-05T12:00:00Z".into(),
            account_id: "acct_123".into(),
            user_id: Some("user_456".into()),
            actor_kind: ActorKind::User,
            action: "license.refreshed".into(),
            sensitivity: Sensitivity::Standard,
            payload: serde_json::json!({ "plan": "pro" }),
        }
    }

    fn sensitive_event() -> AuditEvent {
        AuditEvent {
            event_id: "evt_002".into(),
            occurred_at: "2026-06-05T12:01:00Z".into(),
            account_id: "acct_123".into(),
            user_id: None,
            actor_kind: ActorKind::System,
            action: "billing.subscription_created".into(),
            sensitivity: Sensitivity::Sensitive,
            payload: serde_json::Value::Null,
        }
    }

    // ---- behavior tests ----------------------------------------------------

    #[test]
    fn standard_event_is_not_sensitive() {
        assert!(!standard_event().is_sensitive());
    }

    #[test]
    fn sensitive_event_is_sensitive() {
        assert!(sensitive_event().is_sensitive());
    }

    #[test]
    fn event_round_trips_through_json() {
        let original = standard_event();
        let json = serde_json::to_string(&original).unwrap();
        let recovered: AuditEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn system_event_without_user_id_round_trips() {
        // user_id is optional; absent for system events.
        let ev = sensitive_event();
        assert!(ev.user_id.is_none());
        let json = serde_json::to_string(&ev).unwrap();
        let recovered: AuditEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, recovered);
    }

    #[test]
    fn actor_kind_wire_strings_are_stable() {
        // These are part of the storage contract — they must not drift.
        assert_eq!(serde_json::to_string(&ActorKind::User).unwrap(), "\"user\"");
        assert_eq!(
            serde_json::to_string(&ActorKind::System).unwrap(),
            "\"system\""
        );
    }

    #[test]
    fn sensitivity_wire_strings_are_stable() {
        assert_eq!(
            serde_json::to_string(&Sensitivity::Standard).unwrap(),
            "\"standard\""
        );
        assert_eq!(
            serde_json::to_string(&Sensitivity::Sensitive).unwrap(),
            "\"sensitive\""
        );
    }

    #[test]
    fn null_payload_is_omitted_from_json() {
        let ev = sensitive_event(); // payload is Null
        let json = serde_json::to_string(&ev).unwrap();
        assert!(
            !json.contains("payload"),
            "null payload must be omitted; got: {json}"
        );
    }

    #[test]
    fn present_payload_is_included_in_json() {
        let ev = standard_event(); // payload has content
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains("payload"), "payload must appear; got: {json}");
    }
}
