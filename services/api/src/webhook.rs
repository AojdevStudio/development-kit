//! Stripe webhook ingestion: `POST /webhooks/stripe` (issue #32).
//!
//! Stripe owns the billing lifecycle (ADR-0001/CONTEXT); this endpoint is how its
//! events become authoritative backend state. The handler does four things, in
//! order, and nothing else:
//!
//! 1. **Verify the signature** behind the [`WebhookVerifier`] seam. The mock
//!    accepts a fixed signature so dev and CI exercise the full path with NO live
//!    Stripe webhook secret (issue #32 "mock mode"); the real verifier is the same
//!    trait, HMAC-checking the raw body against the signing secret.
//! 2. **Dedup by event id** via [`ProcessedEventStore`]. Stripe delivers
//!    at-least-once and redelivers on any non-2xx, so the SAME event id can arrive
//!    twice — it must produce exactly one state effect. This is the load-bearing
//!    requirement.
//! 3. **Reconcile** — derive an [`AccountState`] from the typed event and persist
//!    it through the [`MutableAccountStateStore`] write seam, so the entitlement
//!    engine (reading the same store) reflects the billing change.
//! 4. **Acknowledge** with `200` so Stripe stops redelivering — including for
//!    unknown event types and already-processed ids, which are safe no-ops.
//!
//! Everything Stripe-specific that *can* be pure (signature acceptance is mocked;
//! event parsing and the event→state derivation) is a pure function over typed
//! inputs, unit-tested without a router. The account and plan come from the signed
//! event payload, never a user bearer credential — webhooks are authenticated by
//! signature, not by session.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::Router;

use shared::{PlanTier, SubscriptionStatus};

use crate::entitlement::{AccountState, MutableAccountStateStore};

/// The signature the [`MockWebhookVerifier`] accepts. Any other value (or none)
/// is rejected, so tests cover both "valid signature reconciles" and "invalid
/// signature rejected" with no live secret in play.
pub const MOCK_VALID_SIGNATURE: &str = "mock_valid_signature";

/// The header Stripe sends the signature in. Read by the handler and passed to
/// the verifier alongside the raw body.
const SIGNATURE_HEADER: &str = "stripe-signature";

// ---------------------------------------------------------------------------
// Signature verification seam
// ---------------------------------------------------------------------------

/// Why a webhook was rejected before any state was touched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebhookError {
    /// The `Stripe-Signature` header was missing or did not verify against the
    /// raw body. Mapped to `400` — the request is malformed/untrusted, and a
    /// non-2xx tells Stripe to redeliver (a transient signing-clock skew can
    /// self-heal; a forged one keeps failing harmlessly).
    InvalidSignature,
    /// The body was not a JSON object with the `id`/`type` an event must carry.
    /// Mapped to `400`.
    MalformedPayload,
}

/// The Stripe signature seam. A small interface over a deep implementation: the
/// handler asks one question — "is this raw body validly signed?" — and never
/// reasons about HMAC or the signing secret. The mock and the real verifier are
/// interchangeable behind this trait (DIP), so the full ingest path is exercised
/// with NO live secret.
pub trait WebhookVerifier: Send + Sync {
    /// Verify `payload` (the exact raw request body bytes) against the value of
    /// the `Stripe-Signature` header. `Ok(())` means trusted; any failure is
    /// [`WebhookError::InvalidSignature`].
    fn verify(&self, payload: &[u8], signature: Option<&str>) -> Result<(), WebhookError>;
}

/// Deterministic, secret-free verifier for dev and tests.
///
/// Accepts exactly [`MOCK_VALID_SIGNATURE`] and rejects everything else
/// (including a missing header). Pure over its inputs — no clock, no HMAC, no key
/// — so the full webhook path is testable without a Stripe webhook secret (issue
/// #32 "mock mode works without a live provider").
#[derive(Debug, Clone, Default)]
pub struct MockWebhookVerifier;

impl MockWebhookVerifier {
    /// Construct the mock verifier. No secret — that is the point.
    pub fn new() -> Self {
        Self
    }
}

impl WebhookVerifier for MockWebhookVerifier {
    fn verify(&self, _payload: &[u8], signature: Option<&str>) -> Result<(), WebhookError> {
        match signature {
            Some(MOCK_VALID_SIGNATURE) => Ok(()),
            _ => Err(WebhookError::InvalidSignature),
        }
    }
}

/// The real Stripe webhook verifier — the live seam behind the same trait.
///
/// A compile-only stub today, mirroring [`crate::billing::StripeBillingProvider`]:
/// it carries where the webhook signing secret will live and rejects every
/// signature until the live HMAC verification lands in its own issue. Deliberately
/// NOT wired into any default app or test, so the build needs no Stripe secret and
/// the gate stays green. When the live integration lands, `verify` recomputes the
/// `t`/`v1` HMAC-SHA256 over `timestamp.payload` with the signing secret and does
/// a constant-time compare against the header — the trait and handler do not
/// change.
#[derive(Debug, Clone)]
pub struct StripeWebhookVerifier {
    /// The Stripe webhook signing secret (`whsec_…`). Held only here, in the
    /// backend, never serialized, never sent to the desktop (ADR-0001/0002).
    _signing_secret: String,
}

impl StripeWebhookVerifier {
    /// Construct the real verifier from a webhook signing secret. Present so the
    /// production wiring has a constructor; the live HMAC check lands in a
    /// follow-up issue.
    pub fn new(signing_secret: impl Into<String>) -> Self {
        Self {
            _signing_secret: signing_secret.into(),
        }
    }
}

impl WebhookVerifier for StripeWebhookVerifier {
    fn verify(&self, _payload: &[u8], _signature: Option<&str>) -> Result<(), WebhookError> {
        // Live HMAC-SHA256 verification lands in a follow-up issue; until then the
        // real verifier trusts nothing rather than silently accepting.
        Err(WebhookError::InvalidSignature)
    }
}

// ---------------------------------------------------------------------------
// Idempotency: processed-event dedup store
// ---------------------------------------------------------------------------

/// Records which Stripe event ids have already been processed, so a redelivered
/// event is a no-op.
///
/// The walking-skeleton implementation is an in-memory set (the durable
/// implementation is a unique `processed_events(event_id)` row in Postgres, where
/// the insert's uniqueness *is* the dedup). [`mark_if_new`] is the atomic
/// test-and-set the idempotency guarantee rests on: it returns `true` exactly
/// once per id, even under concurrent redelivery, because the check and the
/// insert happen under one lock.
#[derive(Debug, Default)]
pub struct ProcessedEventStore {
    seen: Mutex<HashSet<String>>,
}

impl ProcessedEventStore {
    /// An empty store — no event has been processed yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Atomically record `event_id` as processed, returning `true` if this is the
    /// FIRST time the id is seen (caller should reconcile) and `false` if it was
    /// already recorded (caller should treat as a no-op). The test-and-set is a
    /// single critical section so two concurrent deliveries of one id cannot both
    /// observe "new".
    pub fn mark_if_new(&self, event_id: &str) -> bool {
        self.seen
            .lock()
            .expect("processed-event store mutex poisoned")
            .insert(event_id.to_string())
    }

    /// Whether `event_id` has been recorded as processed. Read-only; the
    /// dedup decision uses [`mark_if_new`], not this.
    pub fn contains(&self, event_id: &str) -> bool {
        self.seen
            .lock()
            .expect("processed-event store mutex poisoned")
            .contains(event_id)
    }
}

// ---------------------------------------------------------------------------
// Event parsing + reconcile (pure)
// ---------------------------------------------------------------------------

/// A Stripe event reduced to the typed fields the reconciler acts on. Parsing
/// happens once, at the boundary, into closed types so the reconcile match is
/// total and an unknown event type or plan string is rejected at the edge rather
/// than mishandled downstream.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BillingEvent {
    /// The Stripe event id (`evt_…`) — the idempotency key.
    id: String,
    /// The reconcile action this event maps to. `None` for an event *type* the
    /// reconciler does not act on (acknowledged, no state change).
    action: Option<ReconcileAction>,
}

/// The state change a recognized billing event implies. Each variant carries the
/// already-typed plan/status/period it derives — no `&str` leaks past parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ReconcileAction {
    /// `checkout.session.completed`: the account starts (or resumes) an active
    /// subscription on the chosen plan.
    Activate { account_id: String, plan: PlanTier },
    /// `customer.subscription.updated`/`.deleted`: set the account's plan, status
    /// and period boundary to the event's reported values.
    SetSubscription {
        account_id: String,
        plan: PlanTier,
        status: SubscriptionStatus,
        current_period_end: Option<u64>,
    },
}

/// Map a Stripe plan string to the typed [`PlanTier`]. Returns `None` for an
/// unrecognized plan so the event is treated as un-actionable rather than
/// defaulted — never a blanket `From<&str>` (a typo must not silently become
/// `Free`).
fn parse_plan(plan: &str) -> Option<PlanTier> {
    match plan {
        "free" => Some(PlanTier::Free),
        "starter" => Some(PlanTier::Starter),
        "pro" => Some(PlanTier::Pro),
        "team" => Some(PlanTier::Team),
        "enterprise" => Some(PlanTier::Enterprise),
        _ => None,
    }
}

/// Map a Stripe subscription status string to the typed [`SubscriptionStatus`].
/// `None` for an unrecognized status — explicit, never a defaulting conversion.
fn parse_status(status: &str) -> Option<SubscriptionStatus> {
    match status {
        "trialing" => Some(SubscriptionStatus::Trialing),
        "active" => Some(SubscriptionStatus::Active),
        "past_due" => Some(SubscriptionStatus::PastDue),
        "canceled" => Some(SubscriptionStatus::Canceled),
        "paused" => Some(SubscriptionStatus::Paused),
        "incomplete" => Some(SubscriptionStatus::Incomplete),
        // Stripe's `unpaid`/`incomplete_expired` map to no paid access; treat as
        // canceled so access collapses to free rather than silently persisting.
        "unpaid" | "incomplete_expired" => Some(SubscriptionStatus::Canceled),
        _ => None,
    }
}

/// The account id an event targets: `client_reference_id` (set on checkout) or
/// `metadata.account_id` (set on the subscription). Either is the backend's own
/// account id, round-tripped through Stripe — never a client-supplied value at
/// request time.
fn account_id_of(object: &serde_json::Value) -> Option<String> {
    object
        .get("client_reference_id")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            object
                .get("metadata")
                .and_then(|m| m.get("account_id"))
                .and_then(serde_json::Value::as_str)
        })
        .map(str::to_string)
}

/// Parse a raw Stripe event body into a typed [`BillingEvent`].
///
/// Pure and total: a body missing `id`/`type` is [`WebhookError::MalformedPayload`];
/// a recognized type with missing/invalid fields yields `action: None` (ignored
/// safely) rather than a panic or a guessed default; an unrecognized type yields
/// `action: None` too.
fn parse_event(payload: &[u8]) -> Result<BillingEvent, WebhookError> {
    let root: serde_json::Value =
        serde_json::from_slice(payload).map_err(|_| WebhookError::MalformedPayload)?;

    let id = root
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or(WebhookError::MalformedPayload)?
        .to_string();
    let event_type = root
        .get("type")
        .and_then(serde_json::Value::as_str)
        .ok_or(WebhookError::MalformedPayload)?;

    let object = root
        .get("data")
        .and_then(|d| d.get("object"))
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    let action = derive_action(event_type, &object);
    Ok(BillingEvent { id, action })
}

/// Derive the reconcile action for a recognized event type, or `None` for a type
/// we ignore or one whose payload lacks the fields the action needs.
fn derive_action(event_type: &str, object: &serde_json::Value) -> Option<ReconcileAction> {
    match event_type {
        "checkout.session.completed" => {
            let account_id = account_id_of(object)?;
            let plan = object
                .get("metadata")
                .and_then(|m| m.get("plan"))
                .and_then(serde_json::Value::as_str)
                .and_then(parse_plan)?;
            Some(ReconcileAction::Activate { account_id, plan })
        }
        "customer.subscription.updated" | "customer.subscription.deleted" => {
            let account_id = account_id_of(object)?;
            let plan = object
                .get("metadata")
                .and_then(|m| m.get("plan"))
                .and_then(serde_json::Value::as_str)
                .and_then(parse_plan)?;
            let status = object
                .get("status")
                .and_then(serde_json::Value::as_str)
                .and_then(parse_status)?;
            let current_period_end = object
                .get("current_period_end")
                .and_then(serde_json::Value::as_u64);
            Some(ReconcileAction::SetSubscription {
                account_id,
                plan,
                status,
                current_period_end,
            })
        }
        // Any other event type is acknowledged but not acted on.
        _ => None,
    }
}

/// Apply a reconcile action to the account-state store. The single write point:
/// derive the [`AccountState`] the action implies and persist it so the next
/// entitlement read reflects the billing change.
fn apply_action(action: &ReconcileAction, accounts: &dyn MutableAccountStateStore) {
    match action {
        ReconcileAction::Activate { account_id, plan } => {
            accounts.set_account_state(
                account_id,
                AccountState {
                    plan: *plan,
                    status: SubscriptionStatus::Active,
                    trial: false,
                    current_period_end: None,
                },
            );
        }
        ReconcileAction::SetSubscription {
            account_id,
            plan,
            status,
            current_period_end,
        } => {
            accounts.set_account_state(
                account_id,
                AccountState {
                    plan: *plan,
                    status: *status,
                    trial: false,
                    current_period_end: *current_period_end,
                },
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Router + handler
// ---------------------------------------------------------------------------

/// Shared state for the webhook route: the account-state store to reconcile into,
/// the processed-event store for dedup, and the signature verifier. All behind
/// `Arc<dyn …>`/`Arc<…>` so the durable Postgres-backed stores and the real
/// Stripe verifier drop in without touching the handler.
#[derive(Clone)]
pub struct WebhookState {
    /// The write seam reconciled state is persisted through (and read back from).
    pub accounts: Arc<dyn MutableAccountStateStore>,
    /// The idempotency dedup store, keyed on Stripe event id.
    pub processed: Arc<ProcessedEventStore>,
    /// The signature verifier (mock in dev/test, real Stripe in production).
    pub verifier: Arc<dyn WebhookVerifier>,
}

/// Route for `POST /webhooks/stripe`, carrying its own [`WebhookState`]. Returned
/// as a `Router<()>` (state applied) so it merges into the app router alongside
/// the other routes, none of which it touches.
pub fn router(state: WebhookState) -> Router {
    Router::new()
        .route("/webhooks/stripe", post(ingest))
        .with_state(state)
}

/// `POST /webhooks/stripe` handler — verify, dedup, reconcile, acknowledge.
///
/// Reads the raw body as bytes (the signature is over the exact bytes, so we must
/// not re-serialize a parsed value), verifies the signature, then on first sight
/// of the event id reconciles its action into account state. Returns `200` for a
/// processed event, an ignored event type, and an already-seen id (all safe
/// no-ops Stripe should stop redelivering); `400` for a bad signature or
/// malformed payload.
async fn ingest(
    State(state): State<WebhookState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> StatusCode {
    let signature = headers.get(SIGNATURE_HEADER).and_then(|v| v.to_str().ok());

    // 1) Signature: an untrusted body never reaches parsing or state.
    if state.verifier.verify(&body, signature).is_err() {
        return StatusCode::BAD_REQUEST;
    }

    // 2) Parse into typed form; a malformed-but-signed body is a 400.
    let event = match parse_event(&body) {
        Ok(event) => event,
        Err(_) => return StatusCode::BAD_REQUEST,
    };

    // 3) Dedup: mark_if_new is the atomic test-and-set. A redelivered id returns
    //    false here and we skip reconcile — exactly one state effect per id.
    if !state.processed.mark_if_new(&event.id) {
        return StatusCode::OK;
    }

    // 4) Reconcile the action (None = recognized-but-no-op or ignored type).
    if let Some(action) = &event.action {
        apply_action(action, state.accounts.as_ref());
    }

    StatusCode::OK
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entitlement::InMemoryAccountStateStore;

    // --- verifier ---

    #[test]
    fn mock_verifier_accepts_only_the_known_signature() {
        let v = MockWebhookVerifier::new();
        assert_eq!(v.verify(b"body", Some(MOCK_VALID_SIGNATURE)), Ok(()));
        assert_eq!(
            v.verify(b"body", Some("nope")),
            Err(WebhookError::InvalidSignature)
        );
        assert_eq!(v.verify(b"body", None), Err(WebhookError::InvalidSignature));
    }

    #[test]
    fn real_verifier_rejects_until_wired() {
        // The real seam exists behind the SAME trait, compiles without a secret,
        // and must never silently accept until the live HMAC check lands.
        let v = StripeWebhookVerifier::new("whsec_placeholder");
        assert_eq!(
            v.verify(b"body", Some("t=1,v1=anything")),
            Err(WebhookError::InvalidSignature)
        );
    }

    // --- dedup store ---

    #[test]
    fn mark_if_new_is_true_once_then_false() {
        let store = ProcessedEventStore::new();
        assert!(store.mark_if_new("evt_1"), "first sight is new");
        assert!(!store.mark_if_new("evt_1"), "second sight is a duplicate");
        assert!(store.contains("evt_1"));
        assert!(!store.contains("evt_2"));
    }

    // --- parsing ---

    #[test]
    fn parse_event_rejects_non_object_and_missing_fields() {
        assert_eq!(
            parse_event(b"not json"),
            Err(WebhookError::MalformedPayload)
        );
        assert_eq!(parse_event(b"{}"), Err(WebhookError::MalformedPayload));
        // Has id but no type.
        assert_eq!(
            parse_event(br#"{"id":"evt_1"}"#),
            Err(WebhookError::MalformedPayload)
        );
    }

    #[test]
    fn parse_checkout_completed_yields_activate_with_typed_plan() {
        let body = br#"{
            "id": "evt_c",
            "type": "checkout.session.completed",
            "data": { "object": {
                "client_reference_id": "acct_x",
                "metadata": { "plan": "team" }
            } }
        }"#;
        let event = parse_event(body).unwrap();
        assert_eq!(event.id, "evt_c");
        assert_eq!(
            event.action,
            Some(ReconcileAction::Activate {
                account_id: "acct_x".into(),
                plan: PlanTier::Team,
            })
        );
    }

    #[test]
    fn parse_subscription_updated_yields_typed_status_and_period() {
        let body = br#"{
            "id": "evt_u",
            "type": "customer.subscription.updated",
            "data": { "object": {
                "metadata": { "account_id": "acct_y", "plan": "pro" },
                "status": "past_due",
                "current_period_end": 1700000000
            } }
        }"#;
        let event = parse_event(body).unwrap();
        assert_eq!(
            event.action,
            Some(ReconcileAction::SetSubscription {
                account_id: "acct_y".into(),
                plan: PlanTier::Pro,
                status: SubscriptionStatus::PastDue,
                current_period_end: Some(1_700_000_000),
            })
        );
    }

    #[test]
    fn parse_unknown_event_type_yields_no_action() {
        let body = br#"{"id":"evt_z","type":"payment_intent.created","data":{"object":{}}}"#;
        let event = parse_event(body).unwrap();
        assert_eq!(
            event.action, None,
            "unknown type is acknowledged, not acted on"
        );
    }

    #[test]
    fn parse_rejects_unknown_plan_string_rather_than_defaulting() {
        // A plan Stripe should never send for this product must NOT silently
        // become Free — the action is dropped (acknowledged, no state change).
        let body = br#"{
            "id": "evt_bad_plan",
            "type": "checkout.session.completed",
            "data": { "object": {
                "client_reference_id": "acct_x",
                "metadata": { "plan": "ultra_mega" }
            } }
        }"#;
        assert_eq!(parse_event(body).unwrap().action, None);
    }

    // --- reconcile (apply_action writes typed state) ---

    #[test]
    fn apply_activate_writes_active_subscription_on_the_plan() {
        let store = InMemoryAccountStateStore::new();
        apply_action(
            &ReconcileAction::Activate {
                account_id: "acct_a".into(),
                plan: PlanTier::Pro,
            },
            &store,
        );
        let state = store.account_state("acct_a").unwrap();
        assert_eq!(state.plan, PlanTier::Pro);
        assert_eq!(state.status, SubscriptionStatus::Active);
    }

    #[test]
    fn apply_set_subscription_writes_status_and_period() {
        let store = InMemoryAccountStateStore::new();
        apply_action(
            &ReconcileAction::SetSubscription {
                account_id: "acct_b".into(),
                plan: PlanTier::Pro,
                status: SubscriptionStatus::Canceled,
                current_period_end: Some(1_700_000_000),
            },
            &store,
        );
        let state = store.account_state("acct_b").unwrap();
        assert_eq!(state.status, SubscriptionStatus::Canceled);
        assert_eq!(state.current_period_end, Some(1_700_000_000));
    }
}
