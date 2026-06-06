//! An in-memory [`PrincipalStore`].
//!
//! This is the walking-skeleton identity backing for `GET /me`: a fixed map of
//! bearer tokens to principals. It exists so the auth/account path is real and
//! end-to-end testable *now*, before the durable Postgres-backed store (a later
//! issue, per the data model in `docs/TAURI-STRIPE-SAAS-ARCHITECTURE.md`)
//! exists. Because the endpoint depends on the [`PrincipalStore`] trait, that
//! swap changes nothing above this line.

use std::collections::HashMap;

use crate::auth::PrincipalStore;
use crate::principal::{Principal, Role};

/// The bearer token the walking-skeleton dev server recognises. The desktop dev
/// build sends this token; both sides share the constant so the dev round-trip
/// works out of the box. This is dev scaffolding, not an authority — real
/// sign-in/session issuance lands in a later issue.
pub const DEV_TOKEN: &str = "tok_alice";

/// A principal store backed by a fixed in-memory token map.
#[derive(Debug, Clone, Default)]
pub struct InMemoryPrincipalStore {
    by_token: HashMap<String, Principal>,
}

impl InMemoryPrincipalStore {
    /// An empty store. Every token resolves to `None` (rejected as unknown).
    pub fn new() -> Self {
        Self::default()
    }

    /// The walking-skeleton dev store: [`DEV_TOKEN`] resolves to a single seed
    /// principal so the runnable server and the desktop dev build demonstrate
    /// the full `GET /me` path without a database. The durable Postgres-backed
    /// store replaces this in a later issue.
    pub fn dev_seed() -> Self {
        Self::new().with_token(DEV_TOKEN, dev_principal())
    }

    /// Bind `token` to `principal`, replacing any existing binding. Chainable so
    /// a store can be seeded in one expression.
    pub fn with_token(mut self, token: impl Into<String>, principal: Principal) -> Self {
        self.by_token.insert(token.into(), principal);
        self
    }
}

/// The principal [`DEV_TOKEN`] resolves to in the walking skeleton.
pub fn dev_principal() -> Principal {
    Principal {
        user_id: "user_alice".into(),
        email: "alice@example.com".into(),
        account_id: "acct_acme".into(),
        account_name: "Acme".into(),
        role: Role::Owner,
    }
}

impl PrincipalStore for InMemoryPrincipalStore {
    fn resolve(&self, token: &str) -> Option<Principal> {
        self.by_token.get(token).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::principal::Role;

    fn alice() -> Principal {
        Principal {
            user_id: "user_alice".into(),
            email: "alice@example.com".into(),
            account_id: "acct_acme".into(),
            account_name: "Acme".into(),
            role: Role::Owner,
        }
    }

    #[test]
    fn empty_store_resolves_nothing() {
        let store = InMemoryPrincipalStore::new();
        assert_eq!(store.resolve("anything"), None);
    }

    #[test]
    fn seeded_token_resolves_to_its_principal() {
        let store = InMemoryPrincipalStore::new().with_token("tok_alice", alice());
        assert_eq!(store.resolve("tok_alice"), Some(alice()));
    }

    #[test]
    fn unknown_token_resolves_to_none() {
        let store = InMemoryPrincipalStore::new().with_token("tok_alice", alice());
        assert_eq!(store.resolve("tok_other"), None);
    }
}
