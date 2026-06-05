//! The resolved caller identity: who is calling and which account they belong to.
//!
//! A `Principal` is the backend's answer to "who is this request?" — the user,
//! the account, the membership, and the role, resolved from an authenticated
//! request. Per ADR-0001 this resolution is a cloud-authority decision; the
//! desktop app only *reads* the result, it never derives identity itself.
//!
//! This is a server-owned response projection, not a `shared` wire primitive:
//! `shared` carries cross-boundary types (feature keys, entitlements, license
//! tokens), while the resolved principal is computed by — and lives in — the
//! cloud API.

use serde::{Deserialize, Serialize};

/// The role a user holds within an account membership.
///
/// String values are the wire contract the desktop reads, so they are snake_case
/// and stable. Owner is the account creator; admin and member are the baseline
/// collaborator roles. Product modules layer their own authorization on top of
/// these without renaming them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Owner,
    Admin,
    Member,
}

impl Role {
    /// The stable wire string for this role.
    pub const fn as_str(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Admin => "admin",
            Role::Member => "member",
        }
    }
}

/// The resolved caller: the user, their account, and the membership role that
/// binds them. This is the shape `GET /me` returns and the unit every
/// authenticated endpoint resolves before doing authority work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    pub user_id: String,
    pub email: String,
    pub account_id: String,
    pub account_name: String,
    pub role: Role,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_wire_strings_are_stable_snake_case() {
        assert_eq!(Role::Owner.as_str(), "owner");
        assert_eq!(Role::Admin.as_str(), "admin");
        assert_eq!(Role::Member.as_str(), "member");
    }

    #[test]
    fn role_serde_representation_matches_as_str() {
        for role in [Role::Owner, Role::Admin, Role::Member] {
            let json = serde_json::to_string(&role).unwrap();
            assert_eq!(json, format!("\"{}\"", role.as_str()));
        }
    }

    #[test]
    fn principal_round_trips_through_json() {
        let principal = Principal {
            user_id: "user_123".into(),
            email: "alice@example.com".into(),
            account_id: "acct_123".into(),
            account_name: "Acme".into(),
            role: Role::Owner,
        };
        let json = serde_json::to_string(&principal).unwrap();
        let back: Principal = serde_json::from_str(&json).unwrap();
        assert_eq!(principal, back);
    }
}
