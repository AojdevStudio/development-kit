//! Behavior: `POST /webhooks/stripe` ingests Stripe billing events and
//! idempotently reconciles account/subscription state, so the entitlement engine
//! reflects billing changes (issue #32 acceptance criteria).
//!
//! Exercised through the real router via `tower::ServiceExt::oneshot` — real
//! requests flow through the real signature verification (behind a mock verifier,
//! so NO live Stripe webhook secret is required), the real dedup store, the real
//! reconciler, and the real account-state store, no socket bound.
//!
//! Authority is server-side (ADR-0001): Stripe is the billing lifecycle owner;
//! the backend ingests its events and is the sole writer of account state. The
//! desktop never sees this endpoint.
//!
//! The load-bearing requirement is idempotency: the *same* event id delivered
//! twice must produce exactly one state effect (Stripe redelivers on any
//! non-2xx, and at-least-once delivery is the contract).

use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use shared::{PlanTier, SubscriptionStatus};
use tower::ServiceExt;

use api::entitlement::{AccountState, InMemoryAccountStateStore};
use api::webhook::{
    MockWebhookVerifier, ProcessedEventStore, StripeWebhookVerifier, WebhookState,
    MOCK_VALID_SIGNATURE,
};

/// The Stripe signature header name. The handler reads the signature from here
/// and hands it to the verifier; the mock verifier accepts exactly
/// [`MOCK_VALID_SIGNATURE`].
const STRIPE_SIG_HEADER: &str = "stripe-signature";

/// The account the fixtures reconcile against. The webhook resolves the account
/// from the event payload (the Stripe customer/metadata), never from a bearer
/// credential — webhooks are unauthenticated-from-the-user's-view, authenticated
/// by signature.
const ACCT: &str = "acct_acme";

/// A `checkout.session.completed` fixture for `ACCT` that activates a Pro plan.
fn checkout_completed_pro(event_id: &str) -> String {
    format!(
        r#"{{
          "id": "{event_id}",
          "type": "checkout.session.completed",
          "data": {{ "object": {{
            "client_reference_id": "{ACCT}",
            "metadata": {{ "plan": "pro" }},
            "subscription": "sub_123"
          }} }}
        }}"#
    )
}

/// A `checkout.session.completed` fixture for an arbitrary account that activates
/// a Pro plan. Used to prove a forged event leaves NO state for a fresh account.
fn checkout_completed_for(account_id: &str, event_id: &str) -> String {
    format!(
        r#"{{
          "id": "{event_id}",
          "type": "checkout.session.completed",
          "data": {{ "object": {{
            "client_reference_id": "{account_id}",
            "metadata": {{ "plan": "pro" }}
          }} }}
        }}"#
    )
}

/// A `customer.subscription.updated` fixture moving `ACCT` to past-due.
fn subscription_updated_past_due(event_id: &str) -> String {
    format!(
        r#"{{
          "id": "{event_id}",
          "type": "customer.subscription.updated",
          "data": {{ "object": {{
            "metadata": {{ "account_id": "{ACCT}", "plan": "pro" }},
            "status": "past_due",
            "current_period_end": 1700000000
          }} }}
        }}"#
    )
}

/// A `customer.subscription.deleted` fixture canceling `ACCT`'s subscription.
fn subscription_deleted(event_id: &str) -> String {
    format!(
        r#"{{
          "id": "{event_id}",
          "type": "customer.subscription.deleted",
          "data": {{ "object": {{
            "metadata": {{ "account_id": "{ACCT}", "plan": "pro" }},
            "status": "canceled",
            "current_period_end": 1700000000
          }} }}
        }}"#
    )
}

/// An event type the reconciler does not act on (still well-formed + signed).
fn unknown_event(event_id: &str) -> String {
    format!(
        r#"{{
          "id": "{event_id}",
          "type": "payment_intent.created",
          "data": {{ "object": {{ "id": "pi_1" }} }}
        }}"#
    )
}

/// Build the webhook router over a shared account-state store the test can read
/// back, plus a fresh processed-event store and the mock verifier (accepts
/// [`MOCK_VALID_SIGNATURE`], rejects everything else — NO live secret).
fn app_with_accounts(accounts: Arc<InMemoryAccountStateStore>) -> axum::Router {
    api::webhook_app(WebhookState {
        accounts,
        processed: Arc::new(ProcessedEventStore::new()),
        verifier: Arc::new(MockWebhookVerifier::new()),
    })
}

async fn post_event(app: axum::Router, sig: Option<&str>, body: &str) -> StatusCode {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/webhooks/stripe")
        .header("content-type", "application/json");
    if let Some(value) = sig {
        builder = builder.header(STRIPE_SIG_HEADER, value);
    }
    app.oneshot(builder.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap()
        .status()
}

// --- AC: core events update subscription + entitlement state ---

#[tokio::test]
async fn valid_checkout_completed_reconciles_account_to_active_pro() {
    let accounts = Arc::new(InMemoryAccountStateStore::new());
    let app = app_with_accounts(accounts.clone());

    let status = post_event(
        app,
        Some(MOCK_VALID_SIGNATURE),
        &checkout_completed_pro("evt_checkout_1"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // The reconciler derived AccountState from the event and persisted it, so the
    // entitlement engine (reading the same store) now reports active Pro.
    let state = accounts.account_state(ACCT).expect("account reconciled");
    assert_eq!(state.plan, PlanTier::Pro);
    assert_eq!(state.status, SubscriptionStatus::Active);
}

#[tokio::test]
async fn subscription_updated_reconciles_status_to_past_due() {
    let accounts = Arc::new(InMemoryAccountStateStore::new().with_account(
        ACCT,
        AccountState {
            plan: PlanTier::Pro,
            status: SubscriptionStatus::Active,
            trial: false,
            current_period_end: None,
        },
    ));
    let app = app_with_accounts(accounts.clone());

    let status = post_event(
        app,
        Some(MOCK_VALID_SIGNATURE),
        &subscription_updated_past_due("evt_upd_1"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        accounts.account_state(ACCT).unwrap().status,
        SubscriptionStatus::PastDue
    );
}

#[tokio::test]
async fn subscription_deleted_reconciles_status_to_canceled() {
    let accounts = Arc::new(InMemoryAccountStateStore::new().with_account(
        ACCT,
        AccountState {
            plan: PlanTier::Pro,
            status: SubscriptionStatus::Active,
            trial: false,
            current_period_end: None,
        },
    ));
    let app = app_with_accounts(accounts.clone());

    let status = post_event(
        app,
        Some(MOCK_VALID_SIGNATURE),
        &subscription_deleted("evt_del_1"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let state = accounts.account_state(ACCT).unwrap();
    assert_eq!(state.status, SubscriptionStatus::Canceled);
    assert_eq!(state.current_period_end, Some(1700000000));
}

// --- AC (load-bearing): replaying the same event id changes state only once ---

#[tokio::test]
async fn duplicate_event_id_is_a_no_op() {
    // First the account is activated as Pro. Then a *later* updated-event with a
    // distinct id flips it to past-due. Then we REPLAY the original checkout id —
    // a duplicate id must NOT re-apply, so the state stays past-due.
    let accounts = Arc::new(InMemoryAccountStateStore::new());
    let app = app_with_accounts(accounts.clone());

    // 1) checkout completed → active Pro
    assert_eq!(
        post_event(
            app.clone(),
            Some(MOCK_VALID_SIGNATURE),
            &checkout_completed_pro("evt_dup")
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        accounts.account_state(ACCT).unwrap().status,
        SubscriptionStatus::Active
    );

    // 2) a different event flips status to past-due
    assert_eq!(
        post_event(
            app.clone(),
            Some(MOCK_VALID_SIGNATURE),
            &subscription_updated_past_due("evt_after"),
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        accounts.account_state(ACCT).unwrap().status,
        SubscriptionStatus::PastDue
    );

    // 3) REPLAY the original checkout event id — dedup must make this a no-op, so
    //    the state stays past-due (NOT re-activated). Still a 2xx so Stripe stops
    //    redelivering.
    assert_eq!(
        post_event(
            app.clone(),
            Some(MOCK_VALID_SIGNATURE),
            &checkout_completed_pro("evt_dup")
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        accounts.account_state(ACCT).unwrap().status,
        SubscriptionStatus::PastDue,
        "replaying a processed event id must not re-apply its effect"
    );
}

// --- AC: the provider signature is verified ---

#[tokio::test]
async fn invalid_signature_is_rejected_and_does_not_reconcile() {
    let accounts = Arc::new(InMemoryAccountStateStore::new());
    let app = app_with_accounts(accounts.clone());

    let status = post_event(
        app,
        Some("t=1,v1=forged"),
        &checkout_completed_pro("evt_bad_sig"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // A rejected event must NOT touch state.
    assert_eq!(accounts.account_state(ACCT), None);
}

#[tokio::test]
async fn missing_signature_header_is_rejected() {
    let accounts = Arc::new(InMemoryAccountStateStore::new());
    let app = app_with_accounts(accounts.clone());

    let status = post_event(app, None, &checkout_completed_pro("evt_no_sig")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(accounts.account_state(ACCT), None);
}

// --- AC: unknown event types are ignored safely ---

#[tokio::test]
async fn unknown_event_type_is_acknowledged_without_reconciling() {
    let accounts = Arc::new(InMemoryAccountStateStore::new());
    let app = app_with_accounts(accounts.clone());

    // Well-formed + correctly signed, but a type the reconciler does not act on:
    // acknowledge (2xx so Stripe stops redelivering) but change no state.
    let status = post_event(
        app,
        Some(MOCK_VALID_SIGNATURE),
        &unknown_event("evt_unknown"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(accounts.account_state(ACCT), None);
}

// --- mounted alongside existing routes (issue #32: additive, no regression) ---

#[tokio::test]
async fn default_app_serves_webhook_alongside_existing_routes() {
    // The runnable dev server must serve the webhook AND keep the existing
    // authority surface working on the same app. Proves the merge is additive.
    let webhook_status = api::app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhooks/stripe")
                .header("content-type", "application/json")
                .header(STRIPE_SIG_HEADER, MOCK_VALID_SIGNATURE)
                .body(Body::from(checkout_completed_pro("evt_default_app")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(webhook_status.status(), StatusCode::OK);

    // /health still responds (public, GET).
    let health = api::app()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);

    // /me/entitlements still responds for the dev token.
    let ent = api::app()
        .oneshot(
            Request::builder()
                .uri("/me/entitlements")
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", api::store::DEV_TOKEN),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ent.status(), StatusCode::OK);
}

// --- end-to-end: a reconciled event is reflected by GET /me/entitlements ---

#[tokio::test]
async fn webhook_reconcile_is_reflected_by_entitlements_on_the_same_app() {
    // The dev account starts as active Pro (seeded). A subscription.deleted webhook
    // cancels it with an already-elapsed period, so the entitlement engine must
    // collapse paid access to free — proving the webhook's write is visible to the
    // entitlements read through the SAME app's shared account store (issue #32:
    // "entitlement reflects billing changes").
    use http_body_util::BodyExt;
    use shared::{EntitlementsResponse, FeatureKey};

    let app = api::app();
    let auth = format!("Bearer {}", api::store::DEV_TOKEN);

    // Before: active Pro grants paid features.
    let before = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/me/entitlements")
                .header(header::AUTHORIZATION, &auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let before: EntitlementsResponse =
        serde_json::from_slice(&before.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(before.entitlements.allows(FeatureKey::CloudSync));

    // A subscription.deleted event for the dev account, period already elapsed.
    let cancel = format!(
        r#"{{
          "id": "evt_e2e_cancel",
          "type": "customer.subscription.deleted",
          "data": {{ "object": {{
            "metadata": {{ "account_id": "{ACCT}", "plan": "pro" }},
            "status": "canceled",
            "current_period_end": 1
          }} }}
        }}"#
    );
    assert_eq!(
        post_event(app.clone(), Some(MOCK_VALID_SIGNATURE), &cancel).await,
        StatusCode::OK
    );

    // After: the entitlements read on the SAME app reflects the cancellation.
    let after = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/me/entitlements")
                .header(header::AUTHORIZATION, &auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let after: EntitlementsResponse =
        serde_json::from_slice(&after.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(
        !after.entitlements.allows(FeatureKey::CloudSync),
        "canceled+elapsed must collapse paid access to free after reconcile"
    );
}

// --- #58: the REAL Stripe HMAC verifier, end-to-end through the router ---

/// The webhook signing secret used by the real-verifier fixtures. A test value —
/// the live secret is never in source, and the leak scan (desktop tree) never
/// sees this api-tree test.
const TEST_WHSEC: &str = "whsec_test_fixture_secret";

/// The current unix time, so fixture signatures fall inside the verifier's
/// staleness tolerance window.
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Build a valid Stripe `t=<ts>,v1=<hex>` signature header for `payload` under
/// `TEST_WHSEC`, exactly as Stripe does: HMAC-SHA256 over `"{ts}.{payload}"`.
fn stripe_sign(ts: u64, payload: &[u8]) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let mut mac = Hmac::<Sha256>::new_from_slice(TEST_WHSEC.as_bytes()).unwrap();
    mac.update(ts.to_string().as_bytes());
    mac.update(b".");
    mac.update(payload);
    let hex = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    format!("t={ts},v1={hex}")
}

/// A webhook router wired to the REAL `StripeWebhookVerifier` (test secret), over
/// a shared account store the test can read back.
fn app_with_real_verifier(accounts: Arc<InMemoryAccountStateStore>) -> axum::Router {
    api::webhook_app(WebhookState {
        accounts,
        processed: Arc::new(ProcessedEventStore::new()),
        verifier: Arc::new(StripeWebhookVerifier::new(TEST_WHSEC)),
    })
}

/// ISC-23: a payload signed with the test secret in the real `t/v1` scheme is
/// accepted by the real verifier and reconciles account state end-to-end.
#[tokio::test]
async fn real_verifier_accepts_a_correctly_signed_event_and_reconciles() {
    let accounts = Arc::new(InMemoryAccountStateStore::new());
    let app = app_with_real_verifier(accounts.clone());

    let body = checkout_completed_pro("evt_real_ok");
    let sig = stripe_sign(now(), body.as_bytes());

    let status = post_event(app, Some(&sig), &body).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a correctly-signed event is accepted"
    );
    let state = accounts.account_state(ACCT).expect("account reconciled");
    assert_eq!(state.plan, PlanTier::Pro);
    assert_eq!(state.status, SubscriptionStatus::Active);
}

/// ISC-24: a forged signature is rejected by the real verifier with 400 and NO
/// state effect — the exact attack #58 closes (the live binary trusted a constant
/// before).
#[tokio::test]
async fn real_verifier_rejects_a_forged_signature_with_no_state_effect() {
    let accounts = Arc::new(InMemoryAccountStateStore::new());
    let app = app_with_real_verifier(accounts.clone());

    let body = checkout_completed_pro("evt_real_forged");
    let forged = format!("t={},v1={}", now(), "0".repeat(64));

    let status = post_event(app, Some(&forged), &body).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a forged signature is rejected"
    );
    assert_eq!(
        accounts.account_state(ACCT),
        None,
        "a rejected event must not touch account state"
    );
}

/// ISC-24 (variant): a payload tampered AFTER signing is rejected — the signature
/// is over the exact bytes, so any mutation breaks it.
#[tokio::test]
async fn real_verifier_rejects_a_tampered_body() {
    let accounts = Arc::new(InMemoryAccountStateStore::new());
    let app = app_with_real_verifier(accounts.clone());

    let original = checkout_completed_pro("evt_real_tamper");
    let sig = stripe_sign(now(), original.as_bytes());
    // POST a DIFFERENT body (different account) under the original signature.
    let tampered = original.replace(ACCT, "acct_attacker");

    let status = post_event(app, Some(&sig), &tampered).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(accounts.account_state("acct_attacker"), None);
    assert_eq!(accounts.account_state(ACCT), None);
}

/// ISC-26: the live app builder wired to a Stripe secret mounts the REAL
/// verifier, so a forged signature is rejected (the mock's constant would have
/// been accepted). This proves selection, not just the verifier in isolation.
#[tokio::test]
async fn app_with_stripe_secret_uses_the_real_verifier_and_rejects_a_forgery() {
    let app = api::app_with_stripe_secret(TEST_WHSEC);
    // The EXACT exploit #58 closes: the mock's constant signature, now inert. A
    // checkout for a brand-new account must be REJECTED and leave NO state, so the
    // attacker cannot grant that account a paid plan with the source constant.
    let body = checkout_completed_for("acct_attacker_const", "evt_sel_mock_const");
    let mock_const = post_event(app, Some(MOCK_VALID_SIGNATURE), &body).await;
    assert_eq!(
        mock_const,
        StatusCode::BAD_REQUEST,
        "the real verifier must reject the mock's constant signature"
    );
    // No fallback to mock when the secret is set: the forged event had no effect.
    let after = api::app()
        .oneshot(
            Request::builder()
                .uri("/me/entitlements")
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", api::store::DEV_TOKEN),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(after.status(), StatusCode::OK); // unrelated dev account untouched

    // A correctly-signed event IS accepted by the same real-verifier app.
    let app = api::app_with_stripe_secret(TEST_WHSEC);
    let sig = stripe_sign(now(), body.as_bytes());
    assert_eq!(
        post_event(app, Some(&sig), &body).await,
        StatusCode::OK,
        "a correctly-signed event is accepted through the real-verifier app"
    );
}

/// ISC-26 (cross-check): the default `api::app()` (no secret) still uses the mock
/// verifier, so dev/CI exercise the path without a live secret — selection only
/// flips to real when the secret is configured.
#[tokio::test]
async fn default_app_still_uses_the_mock_verifier_without_a_secret() {
    let body = checkout_completed_pro("evt_default_mock");
    assert_eq!(
        post_event(api::app(), Some(MOCK_VALID_SIGNATURE), &body).await,
        StatusCode::OK,
        "the default dev app accepts the mock's constant (no live secret configured)"
    );
}

/// ISC-25: idempotency still holds with the real verifier — the same correctly-
/// signed event id delivered twice produces exactly one state effect.
#[tokio::test]
async fn real_verifier_preserves_idempotency_on_redelivery() {
    let accounts = Arc::new(InMemoryAccountStateStore::new());
    let app = app_with_real_verifier(accounts.clone());

    // 1) checkout completed → active Pro.
    let body = checkout_completed_pro("evt_real_dup");
    let sig = stripe_sign(now(), body.as_bytes());
    assert_eq!(
        post_event(app.clone(), Some(&sig), &body).await,
        StatusCode::OK
    );
    assert_eq!(
        accounts.account_state(ACCT).unwrap().status,
        SubscriptionStatus::Active
    );

    // 2) a different (validly-signed) event flips status to past-due.
    let upd = subscription_updated_past_due("evt_real_after");
    let upd_sig = stripe_sign(now(), upd.as_bytes());
    assert_eq!(
        post_event(app.clone(), Some(&upd_sig), &upd).await,
        StatusCode::OK
    );
    assert_eq!(
        accounts.account_state(ACCT).unwrap().status,
        SubscriptionStatus::PastDue
    );

    // 3) REPLAY the original signed checkout id — dedup makes it a no-op even with
    //    a valid signature, so state stays past-due (not re-activated).
    assert_eq!(
        post_event(app.clone(), Some(&sig), &body).await,
        StatusCode::OK
    );
    assert_eq!(
        accounts.account_state(ACCT).unwrap().status,
        SubscriptionStatus::PastDue,
        "a redelivered valid event id must not re-apply its effect"
    );
}
