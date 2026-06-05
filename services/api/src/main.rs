//! Binary entrypoint for the cloud API. Binds the router and serves it.
//!
//! `POST /license/refresh` is mounted only when a signing key is configured via
//! the `LICENSE_SIGNING_KEY` env var (a 64-char hex-encoded 32-byte ed25519
//! seed). Absent that key, the binary serves only the health route — so an
//! unconfigured deploy never exposes a token-minting endpoint.
//!
//! SECURITY — AUTH GAP (issue #28 scope): `/license/refresh` currently mints a
//! token for any caller-supplied `account_id`. Authentication and account/
//! entitlement validation are a separate issue (auth `/me`, #20). This route
//! MUST be placed behind that auth middleware before any production deploy;
//! until then, configure the signing key only in trusted/dev environments.

use std::net::SocketAddr;

use api::license::LicenseState;
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

/// Assemble the router, mounting the license route iff a signing key is present.
fn build_router() -> Router {
    match load_signer() {
        Some(signer) => {
            println!("license signing key loaded; mounting POST /license/refresh");
            println!(
                "WARNING: /license/refresh is NOT yet behind auth (issue #28); \
                 do not expose this binary publicly until auth lands (#20)"
            );
            api::app_with_license(LicenseState::new(signer))
        }
        None => {
            println!(
                "LICENSE_SIGNING_KEY not set; serving health only \
                 (POST /license/refresh disabled)"
            );
            api::app()
        }
    }
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
}
