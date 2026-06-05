//! Authentication: turn an incoming request into a resolved [`Principal`], or
//! reject it.
//!
//! Per ADR-0001 identity resolution is a cloud-authority decision. Two concerns
//! live here, kept deliberately separate so each is testable in isolation:
//!
//! 1. **Credential extraction** — pulling the bearer token out of the
//!    `Authorization` header. A missing or malformed header is a rejection, not
//!    a panic.
//! 2. **Principal resolution** — mapping a valid token to the user, account,
//!    membership, and role. This is modeled as the [`PrincipalStore`] trait so
//!    the durable Postgres-backed store (a later issue, per the architecture
//!    doc's data model) can drop in without changing the endpoint or its tests.

use crate::principal::Principal;

/// Why an authenticated request could not be resolved to a principal.
///
/// These map onto HTTP rejections at the boundary; the variants exist so the
/// handler can distinguish "no/!malformed credential" from "credential
/// presented but unknown", which are different failures even though both are
/// rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    /// No `Authorization` header, or it was not a well-formed `Bearer` token.
    MissingCredentials,
    /// A bearer token was presented but did not resolve to a known principal.
    InvalidToken,
}

/// Resolve a bearer token to the principal it authenticates.
///
/// A small interface over a deep implementation (ADR-0002 spirit): callers ask
/// one question — "who is this token?" — and never reason about how identity is
/// stored. The walking-skeleton implementation is in-memory; the production
/// implementation is Postgres-backed and lands in its own issue. Returning
/// `None` (token unknown) is distinct from the resolution being unavailable.
pub trait PrincipalStore: Send + Sync {
    /// Resolve the principal for `token`, or `None` if no principal matches.
    fn resolve(&self, token: &str) -> Option<Principal>;
}

/// Extract the bearer token from an `Authorization` header value.
///
/// Returns the token slice for a well-formed `Bearer <token>` header, or
/// [`AuthError::MissingCredentials`] otherwise. The scheme match is
/// case-insensitive (per RFC 7235) but the token itself is preserved verbatim.
pub fn bearer_token(header: Option<&str>) -> Result<&str, AuthError> {
    let header = header.ok_or(AuthError::MissingCredentials)?;
    let (scheme, token) = header
        .split_once(' ')
        .ok_or(AuthError::MissingCredentials)?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return Err(AuthError::MissingCredentials);
    }
    let token = token.trim();
    if token.is_empty() {
        return Err(AuthError::MissingCredentials);
    }
    Ok(token)
}

/// Resolve a request's `Authorization` header to a principal: extract the bearer
/// token, then look it up in the store. This is the one function endpoints call.
pub fn resolve_principal(
    store: &dyn PrincipalStore,
    auth_header: Option<&str>,
) -> Result<Principal, AuthError> {
    let token = bearer_token(auth_header)?;
    store.resolve(token).ok_or(AuthError::InvalidToken)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::principal::Role;
    use std::collections::HashMap;

    /// In-memory store mapping tokens to principals — the test double and the
    /// walking-skeleton seed both use this shape.
    struct InMemoryStore {
        by_token: HashMap<String, Principal>,
    }

    fn alice() -> Principal {
        Principal {
            user_id: "user_alice".into(),
            email: "alice@example.com".into(),
            account_id: "acct_acme".into(),
            account_name: "Acme".into(),
            role: Role::Owner,
        }
    }

    fn store_with_alice() -> InMemoryStore {
        let mut by_token = HashMap::new();
        by_token.insert("tok_alice".to_string(), alice());
        InMemoryStore { by_token }
    }

    impl PrincipalStore for InMemoryStore {
        fn resolve(&self, token: &str) -> Option<Principal> {
            self.by_token.get(token).cloned()
        }
    }

    #[test]
    fn bearer_token_extracts_well_formed_token() {
        assert_eq!(bearer_token(Some("Bearer tok_alice")), Ok("tok_alice"));
    }

    #[test]
    fn bearer_scheme_match_is_case_insensitive() {
        assert_eq!(bearer_token(Some("bearer tok_alice")), Ok("tok_alice"));
        assert_eq!(bearer_token(Some("BEARER tok_alice")), Ok("tok_alice"));
    }

    #[test]
    fn missing_header_is_missing_credentials() {
        assert_eq!(bearer_token(None), Err(AuthError::MissingCredentials));
    }

    #[test]
    fn wrong_scheme_is_missing_credentials() {
        assert_eq!(
            bearer_token(Some("Basic dXNlcjpwYXNz")),
            Err(AuthError::MissingCredentials)
        );
    }

    #[test]
    fn empty_bearer_value_is_missing_credentials() {
        assert_eq!(
            bearer_token(Some("Bearer ")),
            Err(AuthError::MissingCredentials)
        );
    }

    #[test]
    fn resolve_principal_returns_principal_for_known_token() {
        let store = store_with_alice();
        let resolved = resolve_principal(&store, Some("Bearer tok_alice")).unwrap();
        assert_eq!(resolved, alice());
    }

    #[test]
    fn resolve_principal_rejects_unknown_token_as_invalid() {
        let store = store_with_alice();
        let result = resolve_principal(&store, Some("Bearer tok_unknown"));
        assert_eq!(result, Err(AuthError::InvalidToken));
    }

    #[test]
    fn resolve_principal_rejects_missing_header_as_missing_credentials() {
        let store = store_with_alice();
        let result = resolve_principal(&store, None);
        assert_eq!(result, Err(AuthError::MissingCredentials));
    }
}
