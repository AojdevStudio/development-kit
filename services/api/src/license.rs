//! `POST /license/refresh` — short-lived license-token issuance (issue #28,
//! hardened per issue #56).
//!
//! This is the backend half of the offline-paid-access scheme (ADR-0001):
//! the cloud service mints a signed, short-lived [`LicenseToken`] that the
//! desktop app verifies locally with `license-verify`. The private signing key
//! lives only here, in the backend — never in the desktop tree (ADR-0002).
//!
//! **Authority (issue #56).** The route is mounted behind auth: the caller is
//! resolved from their bearer token (never named in the request body), the
//! `account_id` is taken from the authenticated principal, and the token's
//! `plan`/`features` are sourced from the entitlement engine
//! ([`resolve_entitlements`]) over the account's real billing state — never a
//! hardcoded `"pro"`. A free account therefore receives a free token, and a
//! caller can never mint a token for an account it does not own.
//!
//! The handler is split from its pure core so the issuance logic is testable
//! without binding a socket or building a `Router`:
//!
//! - [`issue_license`] is the pure function: given a signer, a clock, a TTL, the
//!   account, and the entitlements the backend resolved for it, it produces the
//!   wire response. No I/O, fully deterministic under a fixed clock.
//! - [`refresh`] is the Axum adapter that authenticates the caller, resolves
//!   their entitlements, and calls [`issue_license`].

use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::Json;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use license_sign::LicenseSigner;
use shared::{Entitlements, LicenseRefreshResponse, LicenseToken};

use crate::auth::{resolve_principal, AuthError, PrincipalStore};
use crate::entitlement::{resolve_entitlements, AccountState, AccountStateStore};

/// How long an issued license token stays valid. Short by design: the desktop
/// must re-check in with the backend on this cadence, which is what bounds
/// offline access and lets revocation take effect (ADR-0001).
pub const DEFAULT_TOKEN_TTL_SECS: u64 = 7 * 24 * 60 * 60; // 7 days

/// Shared state for `POST /license/refresh`: the backend signer plus the auth and
/// account-state seams the authority decision needs.
///
/// `Arc` so the single signer (and its private key) is shared across all request
/// handlers without copying key material — this is the *only* place the private
/// key is reachable, and it is never serialized into a response. The principal
/// and account-state stores are the same seams the authenticated feature gate and
/// `/me/entitlements` use, so the durable Postgres-backed stores drop in without
/// touching the handler.
#[derive(Clone)]
pub struct LicenseState {
    signer: Arc<LicenseSigner>,
    ttl_secs: u64,
    principals: Arc<dyn PrincipalStore>,
    accounts: Arc<dyn AccountStateStore>,
}

impl LicenseState {
    /// Build the license state from the backend signing key and the auth/account
    /// seams the authority decision resolves the caller and their plan through.
    pub fn new(
        signer: LicenseSigner,
        principals: Arc<dyn PrincipalStore>,
        accounts: Arc<dyn AccountStateStore>,
    ) -> Self {
        Self {
            signer: Arc::new(signer),
            ttl_secs: DEFAULT_TOKEN_TTL_SECS,
            principals,
            accounts,
        }
    }

    /// The public verifying-key bytes the desktop needs to verify issued tokens.
    /// This is the only key material that may leave the backend.
    pub fn verifying_key_bytes(&self) -> [u8; 32] {
        self.signer.verifying_key_bytes()
    }
}

/// Issue a signed, short-lived license token for `account_id`, carrying the
/// plan/features the backend *resolved* for that account.
///
/// Pure over its inputs: `now` (unix epoch seconds) is supplied rather than read
/// from the system clock, and `entitlements` is the engine's already-computed
/// verdict — so a free account's `entitlements.plan` is `Free` and its feature
/// set is the free set, and there is no way for this function to mint a Pro token
/// for a non-Pro account. The returned response carries the Base64-encoded
/// canonical payload bytes and detached signature, plus the absolute expiry the
/// desktop schedules against.
pub fn issue_license(
    signer: &LicenseSigner,
    account_id: &str,
    user_id: &str,
    entitlements: &Entitlements,
    now: u64,
    ttl_secs: u64,
) -> LicenseRefreshResponse {
    let expires_at = now.saturating_add(ttl_secs);
    let token = LicenseToken {
        token_id: format!("lic_{account_id}_{now}"),
        user_id: user_id.to_string(),
        account_id: account_id.to_string(),
        // Plan and features come from the entitlement engine, never a literal.
        plan: entitlements.plan.as_str().to_string(),
        features: entitlements.features.clone(),
        issued_at: now,
        expires_at,
    };

    let signed = signer.sign(&token);
    LicenseRefreshResponse {
        payload_b64: BASE64.encode(&signed.payload),
        signature_b64: BASE64.encode(signed.signature),
        expires_at,
    }
}

/// `POST /license/refresh` handler — authenticate, resolve entitlements, issue.
///
/// 1. Resolve the caller from their bearer token (401 on any auth failure). The
///    request body never names the principal.
/// 2. Resolve the account's entitlements from its real billing state via the
///    engine. An account with no billing state on record resolves to the free
///    entitlement set — never a paid token.
/// 3. Sign a token whose `account_id` is the principal's account and whose
///    plan/features are the engine's verdict.
pub async fn refresh(
    State(state): State<LicenseState>,
    headers: HeaderMap,
) -> Result<Json<LicenseRefreshResponse>, StatusCode> {
    let auth_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    let principal = match resolve_principal(state.principals.as_ref(), auth_header) {
        Ok(p) => p,
        Err(AuthError::MissingCredentials) | Err(AuthError::InvalidToken) => {
            return Err(StatusCode::UNAUTHORIZED)
        }
    };

    let now = now_unix_secs();
    // No billing state on record → resolve the free entitlement set. The engine
    // is the single authority on plan/features; this never fabricates paid access.
    let account_state = state
        .accounts
        .account_state(&principal.account_id)
        .unwrap_or_else(free_account_state);
    let entitlements = resolve_entitlements(principal.account_id.clone(), &account_state, now);

    Ok(Json(issue_license(
        &state.signer,
        &principal.account_id,
        &principal.user_id,
        &entitlements,
        now,
        state.ttl_secs,
    )))
}

/// The billing state used when an account has no record on file: a free plan with
/// no subscription. Resolving this through the engine yields the free feature set,
/// so an unknown account can never be granted paid access.
fn free_account_state() -> AccountState {
    use shared::{PlanTier, SubscriptionStatus};
    AccountState {
        plan: PlanTier::Free,
        status: SubscriptionStatus::Free,
        trial: false,
        current_period_end: None,
    }
}

/// The current wall-clock time in unix epoch seconds. Isolated so the pure
/// issuance path never reads the clock directly.
fn now_unix_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Resolve `Entitlements` for a free account at `now` — the helper the unit tests
/// use to build the engine verdict an issuance path consumes.
#[cfg(test)]
fn free_entitlements(account_id: &str, now: u64) -> Entitlements {
    resolve_entitlements(account_id.to_string(), &free_account_state(), now)
}

#[cfg(test)]
mod tests {
    use super::*;
    // Verify with the raw ed25519 primitive — NOT `license-verify`. The backend
    // crate may not depend on the desktop verifier (ADR-0002); the cross-crate
    // sign→verify round-trip is covered in `license-sign`'s integration test.
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    use shared::{FeatureKey, FeatureValue, PlanTier, SubscriptionStatus};

    fn signer() -> LicenseSigner {
        LicenseSigner::from_secret_bytes(&[7u8; 32]).unwrap()
    }

    fn pro_entitlements(account_id: &str, now: u64) -> Entitlements {
        resolve_entitlements(
            account_id.to_string(),
            &AccountState {
                plan: PlanTier::Pro,
                status: SubscriptionStatus::Active,
                trial: false,
                current_period_end: None,
            },
            now,
        )
    }

    fn decode_signature(resp: &LicenseRefreshResponse) -> Signature {
        let sig_vec = BASE64.decode(&resp.signature_b64).unwrap();
        let sig: [u8; 64] = sig_vec.try_into().unwrap();
        Signature::from_bytes(&sig)
    }

    #[test]
    fn issued_token_has_a_valid_signature_for_the_signer_key() {
        let signer = signer();
        let verifying = VerifyingKey::from_bytes(&signer.verifying_key_bytes()).unwrap();
        let ent = pro_entitlements("acct_123", 1_700_000_000);

        let resp = issue_license(
            &signer,
            "acct_123",
            "user_123",
            &ent,
            1_700_000_000,
            DEFAULT_TOKEN_TTL_SECS,
        );

        let payload = BASE64.decode(&resp.payload_b64).unwrap();
        let signature = decode_signature(&resp);

        verifying
            .verify(&payload, &signature)
            .expect("issued token must verify under the signer's public key");
    }

    #[test]
    fn issued_token_plan_and_features_come_from_the_entitlements() {
        let signer = signer();
        let ent = pro_entitlements("acct_123", 1_700_000_000);
        let resp = issue_license(
            &signer,
            "acct_123",
            "user_123",
            &ent,
            1_700_000_000,
            DEFAULT_TOKEN_TTL_SECS,
        );
        let payload = BASE64.decode(&resp.payload_b64).unwrap();
        let token: LicenseToken = serde_json::from_slice(&payload).unwrap();
        assert_eq!(token.plan, "pro");
        assert_eq!(
            token.features.get(&FeatureKey::AdvancedReports),
            Some(&FeatureValue::Enabled(true))
        );
    }

    #[test]
    fn free_account_entitlements_produce_a_free_token() {
        let signer = signer();
        let ent = free_entitlements("acct_free", 1_700_000_000);
        let resp = issue_license(
            &signer,
            "acct_free",
            "user_free",
            &ent,
            1_700_000_000,
            DEFAULT_TOKEN_TTL_SECS,
        );
        let payload = BASE64.decode(&resp.payload_b64).unwrap();
        let token: LicenseToken = serde_json::from_slice(&payload).unwrap();
        assert_eq!(token.plan, "free", "free account → free plan, never pro");
        assert_ne!(
            token.features.get(&FeatureKey::AdvancedReports),
            Some(&FeatureValue::Enabled(true)),
            "a free token must not enable a Pro-only feature"
        );
    }
}
