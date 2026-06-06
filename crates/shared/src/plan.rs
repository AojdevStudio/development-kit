//! Plan and subscription-status types.
//!
//! Using closed enums here makes invalid states unrepresentable: code that
//! receives an `Entitlements` value from the backend cannot accidentally treat
//! a free-tier account as pro, or an active subscription as canceled, because
//! the type system won't allow constructing that state.

use serde::{Deserialize, Serialize};

/// The subscription plan tier for an account.
///
/// These string values are part of the wire contract — they must stay stable.
/// The `#[serde(rename_all = "snake_case")]` keeps JSON keys lowercase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanTier {
    Free,
    Starter,
    Pro,
    Team,
    Enterprise,
}

impl PlanTier {
    /// The stable wire string for this tier.
    pub const fn as_str(self) -> &'static str {
        match self {
            PlanTier::Free => "free",
            PlanTier::Starter => "starter",
            PlanTier::Pro => "pro",
            PlanTier::Team => "team",
            PlanTier::Enterprise => "enterprise",
        }
    }

    /// Whether this tier includes any paid access. Used by gate code to short-
    /// circuit feature checks without examining individual feature keys.
    pub const fn is_paid(self) -> bool {
        !matches!(self, PlanTier::Free)
    }
}

impl std::fmt::Display for PlanTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The current subscription lifecycle state for an account.
///
/// The backend sets this based on Stripe subscription state and reconciled
/// webhook events. The desktop reads it; it never writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionStatus {
    /// Free account — no Stripe subscription exists.
    Free,
    /// Trial period is active. Trial entitlements apply.
    Trialing,
    /// Subscription is active and payment is current.
    Active,
    /// Payment is past-due. Access degradation policy applies.
    PastDue,
    /// Subscription has been canceled. Free-tier access only.
    Canceled,
    /// Subscription has been paused (if the product supports it).
    Paused,
    /// Subscription is incomplete (initial payment failed).
    Incomplete,
}

impl SubscriptionStatus {
    /// Whether this status grants paid access. `PastDue` is included because
    /// Stripe gives a grace period; the entitlement engine may degrade access
    /// through the features map instead of blanket-revoking.
    pub const fn grants_paid_access(self) -> bool {
        matches!(
            self,
            SubscriptionStatus::Trialing | SubscriptionStatus::Active | SubscriptionStatus::PastDue
        )
    }
}

impl std::fmt::Display for SubscriptionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            SubscriptionStatus::Free => "free",
            SubscriptionStatus::Trialing => "trialing",
            SubscriptionStatus::Active => "active",
            SubscriptionStatus::PastDue => "past_due",
            SubscriptionStatus::Canceled => "canceled",
            SubscriptionStatus::Paused => "paused",
            SubscriptionStatus::Incomplete => "incomplete",
        };
        f.write_str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- PlanTier ---

    #[test]
    fn plan_tier_wire_strings_are_stable() {
        assert_eq!(PlanTier::Free.as_str(), "free");
        assert_eq!(PlanTier::Starter.as_str(), "starter");
        assert_eq!(PlanTier::Pro.as_str(), "pro");
        assert_eq!(PlanTier::Team.as_str(), "team");
        assert_eq!(PlanTier::Enterprise.as_str(), "enterprise");
    }

    #[test]
    fn plan_tier_serde_matches_as_str() {
        for tier in [
            PlanTier::Free,
            PlanTier::Starter,
            PlanTier::Pro,
            PlanTier::Team,
            PlanTier::Enterprise,
        ] {
            let json = serde_json::to_string(&tier).unwrap();
            assert_eq!(json, format!("\"{}\"", tier.as_str()));
        }
    }

    #[test]
    fn plan_tier_round_trips_through_json() {
        for tier in [
            PlanTier::Free,
            PlanTier::Starter,
            PlanTier::Pro,
            PlanTier::Team,
            PlanTier::Enterprise,
        ] {
            let json = serde_json::to_string(&tier).unwrap();
            let back: PlanTier = serde_json::from_str(&json).unwrap();
            assert_eq!(tier, back);
        }
    }

    #[test]
    fn only_free_tier_is_not_paid() {
        assert!(!PlanTier::Free.is_paid());
        assert!(PlanTier::Starter.is_paid());
        assert!(PlanTier::Pro.is_paid());
        assert!(PlanTier::Team.is_paid());
        assert!(PlanTier::Enterprise.is_paid());
    }

    // --- SubscriptionStatus ---

    #[test]
    fn subscription_status_round_trips_through_json() {
        for status in [
            SubscriptionStatus::Free,
            SubscriptionStatus::Trialing,
            SubscriptionStatus::Active,
            SubscriptionStatus::PastDue,
            SubscriptionStatus::Canceled,
            SubscriptionStatus::Paused,
            SubscriptionStatus::Incomplete,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let back: SubscriptionStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, back);
        }
    }

    #[test]
    fn active_and_trialing_grant_paid_access() {
        assert!(SubscriptionStatus::Active.grants_paid_access());
        assert!(SubscriptionStatus::Trialing.grants_paid_access());
        assert!(SubscriptionStatus::PastDue.grants_paid_access());
    }

    #[test]
    fn canceled_and_free_do_not_grant_paid_access() {
        assert!(!SubscriptionStatus::Canceled.grants_paid_access());
        assert!(!SubscriptionStatus::Free.grants_paid_access());
        assert!(!SubscriptionStatus::Paused.grants_paid_access());
        assert!(!SubscriptionStatus::Incomplete.grants_paid_access());
    }
}
