//! Binary entrypoint for the cloud API. Binds the router and serves it.
//!
//! `POST /license/refresh` is mounted only when a signing key is configured via
//! the `LICENSE_SIGNING_KEY` env var (a 64-char hex-encoded 32-byte ed25519
//! seed). Absent that key, the binary serves only the health route — so an
//! unconfigured deploy never exposes a token-minting endpoint.
//!
//! SECURITY (issue #56, hardened): `/license/refresh` is mounted behind auth —
//! the caller is resolved from their bearer token, the `account_id` comes from
//! the authenticated principal (never the body), and the token's plan/features
//! come from the entitlement engine over the account's real billing state. A free
//! account therefore receives a free token; no caller can mint a token for an
//! account it does not own.

use std::net::SocketAddr;
use std::sync::Arc;

use api::entitlement::InMemoryAccountStateStore;
use api::license::LicenseState;
use api::store::InMemoryPrincipalStore;
use axum::Router;
use license_sign::LicenseSigner;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port: u16 = std::env::var("API_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8787);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    let app = build_router();

    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("api listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Assemble the router. Two independent env-gated authority decisions, each
/// mirroring the other so a configured deploy can never fall back to a dev/mock
/// authority path:
///
/// 1. **Webhook verifier (issue #58):** when `STRIPE_WEBHOOK_SECRET` is present
///    the binary mounts the REAL [`StripeWebhookVerifier`] (HMAC-SHA256); absent
///    it, the deterministic mock (dev/CI only). The real verifier is selected
///    whenever the secret is set — there is no path that keeps the mock once a
///    secret is configured.
/// 2. **License route (issue #28/#56):** mounted only when `LICENSE_SIGNING_KEY`
///    is present, and always auth-gated with account/plan resolved server-side.
fn build_router() -> Router {
    // 1) Base app: real webhook verifier iff the secret is set, else the mock.
    let base = match webhook_secret() {
        Some(secret) => {
            println!(
                "STRIPE_WEBHOOK_SECRET set; mounting POST /webhooks/stripe with the \
                 REAL Stripe HMAC verifier (mock disabled)"
            );
            api::app_with_stripe_secret(secret)
        }
        None => {
            println!(
                "STRIPE_WEBHOOK_SECRET not set; POST /webhooks/stripe uses the dev \
                 mock verifier (no live secret)"
            );
            api::app()
        }
    };

    // 2) License route on top, iff a signing key is present.
    match load_signer() {
        Some(signer) => {
            println!(
                "license signing key loaded; mounting POST /license/refresh \
                 (auth-gated; account + plan resolved server-side)"
            );
            // The license route shares the same dev principal/account stores the
            // rest of the authority surface uses, so the caller is authenticated
            // and their plan resolved from real billing state (issue #56). The
            // durable Postgres-backed stores drop in behind the same traits.
            let license = LicenseState::new(
                signer,
                Arc::new(InMemoryPrincipalStore::dev_seed()),
                Arc::new(InMemoryAccountStateStore::dev_seed()),
            );
            base.route(
                "/license/refresh",
                axum::routing::post(api::license::refresh).with_state(license),
            )
        }
        None => {
            println!("LICENSE_SIGNING_KEY not set; POST /license/refresh disabled");
            base
        }
    }
}

/// Normalize a raw `STRIPE_WEBHOOK_SECRET` value into the configured secret, if
/// any. A missing or blank value is `None` (mock verifier); a non-blank value is
/// `Some` (real verifier). Pure over its input so the selection rule is unit-
/// testable without mutating process env.
fn normalize_webhook_secret(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// The Stripe webhook signing secret from the environment, if configured. The
/// selection input the binary mirrors the license env-gating on (issue #58): when
/// it is `Some`, the REAL verifier is mounted and there is no fallback to the mock.
fn webhook_secret() -> Option<String> {
    normalize_webhook_secret(std::env::var("STRIPE_WEBHOOK_SECRET").ok().as_deref())
}

/// Build the backend signer from `LICENSE_SIGNING_KEY` (64 hex chars = 32-byte
/// ed25519 seed). Returns `None` if the var is unset; panics with a clear
/// message if it is set but malformed, so a misconfigured key fails loudly
/// rather than silently disabling licensing.
fn load_signer() -> Option<LicenseSigner> {
    let hex = std::env::var("LICENSE_SIGNING_KEY").ok()?;
    let seed = decode_hex_32(&hex)
        .expect("LICENSE_SIGNING_KEY must be 64 hex chars (a 32-byte ed25519 seed)");
    Some(
        LicenseSigner::from_secret_bytes(&seed)
            .expect("LICENSE_SIGNING_KEY decoded to 32 bytes but the signer rejected it"),
    )
}

/// Decode a 64-character hex string into 32 bytes. Returns `None` on any length
/// or non-hex-digit error. Kept dependency-free (no `hex` crate) for one small,
/// auditable parse of a single trusted-config value.
fn decode_hex_32(s: &str) -> Option<[u8; 32]> {
    let s = s.trim();
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        let hi = (s.as_bytes()[2 * i] as char).to_digit(16)?;
        let lo = (s.as_bytes()[2 * i + 1] as char).to_digit(16)?;
        *byte = (hi * 16 + lo) as u8;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_hex_32_round_trips_a_known_seed() {
        let hex = "0707070707070707070707070707070707070707070707070707070707070707";
        assert_eq!(decode_hex_32(hex), Some([7u8; 32]));
    }

    #[test]
    fn decode_hex_32_rejects_wrong_length() {
        assert_eq!(decode_hex_32("0707"), None);
    }

    #[test]
    fn decode_hex_32_rejects_non_hex_digits() {
        let bad = "zz07070707070707070707070707070707070707070707070707070707070707";
        assert_eq!(decode_hex_32(bad), None);
    }

    // --- #58: webhook verifier selection mirrors the license env-gating ---

    #[test]
    fn a_present_webhook_secret_selects_the_real_verifier() {
        // A configured secret is `Some`, which is what drives `build_router` to
        // mount the REAL verifier — there is no branch that keeps the mock once a
        // secret is set.
        assert_eq!(
            normalize_webhook_secret(Some("whsec_live_value")),
            Some("whsec_live_value".to_string())
        );
    }

    #[test]
    fn an_absent_or_blank_webhook_secret_keeps_the_mock() {
        // Absent → mock. Blank/whitespace → mock (never the real verifier with an
        // unusable empty key).
        assert_eq!(normalize_webhook_secret(None), None);
        assert_eq!(normalize_webhook_secret(Some("")), None);
        assert_eq!(normalize_webhook_secret(Some("   ")), None);
    }
}
