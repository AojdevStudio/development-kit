//! The Notes product's entitlement policy (issue #37 — sample product).
//!
//! This is the product analogue of the spine's [`resolve_entitlements`]
//! (`crate::entitlement`): a pure, total function that maps an account's typed
//! billing [`AccountState`] onto the product's [`ProductEntitlements`] snapshot —
//! the per-product policy table that decides which `notes.*`
//! [`ProductFeatureKey`]s the account is granted.
//!
//! Authority boundary (ADR-0001): paid access is *decided here, on the server*,
//! from the account's real billing state — never read from a request body and
//! never authored by the desktop. The Notes backend route resolves this snapshot
//! from the caller's token (auth → account state → this policy) and gates against
//! it, exactly the way the baseline authenticated gate resolves `Entitlements`.
//! A lying client body can therefore never grant `notes.publish_note`.
//!
//! The Notes tier policy is deliberately tiny: publishing a note to the cloud is
//! the one paid capability, granted to any account whose subscription currently
//! grants paid access (trial, active, or canceled-within-period). Free, past-due,
//! paused, and lapsed accounts are denied — the same access ladder the spine
//! engine walks, asked once for this product key.

use shared::{FeatureValue, ProductEntitlements, ProductFeatureKey};

use crate::entitlement::AccountState;
use crate::products::notes::meta;

/// The Notes paid capability: publish a note to the cloud. Built fresh (rather
/// than held as a `const`, which `ProductFeatureKey`'s validated constructor does
/// not allow) so every site asks the validator for the same well-formed key.
pub fn publish_note_key() -> ProductFeatureKey {
    ProductFeatureKey::new(meta().namespace, "publish_note").expect("valid notes product key")
}

/// Resolve an account's Notes product access from its billing state at `now`.
///
/// Pure and total, mirroring [`crate::entitlement::resolve_entitlements`]: the
/// subscription status decides whether the product's paid capability is granted,
/// and `now` is injected (not read from the wall clock) so the period-boundary
/// scenarios are deterministic in tests. The desktop never runs this — it reads
/// the resulting snapshot.
pub fn resolve_product_entitlements(
    account_id: impl Into<String>,
    state: &AccountState,
    now: u64,
) -> ProductEntitlements {
    let namespace = meta().namespace;
    let snapshot = ProductEntitlements::new(account_id, &namespace);
    // The one Notes paid key is granted exactly when the account's subscription
    // currently grants paid access — the same ladder the spine engine walks.
    snapshot.with(
        publish_note_key(),
        FeatureValue::Enabled(grants_paid_access(state, now)),
    )
}

/// Whether `state` currently grants paid access at `now`. Encapsulates the Notes
/// product's read of the shared subscription ladder so the policy lives in one
/// place: trial and active always grant; canceled grants until the period
/// boundary; everything else denies.
///
/// The canceled-within-period boundary is read here from the public
/// [`AccountState::current_period_end`] field rather than the spine engine's
/// private helper, so the product policy plugs in over the foundation's *public*
/// surface and edits no shared-foundation code (the seam's one rule). The boundary
/// matches the spine: access ends *at* `current_period_end`, so `now < end` still
/// grants.
fn grants_paid_access(state: &AccountState, now: u64) -> bool {
    use shared::SubscriptionStatus::*;
    match state.status {
        Trialing | Active => true,
        Canceled => state.current_period_end.is_some_and(|end| now < end),
        Free | PastDue | Paused | Incomplete => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::{PlanTier, SubscriptionStatus};

    const NOW: u64 = 1_700_000_000;

    fn state(plan: PlanTier, status: SubscriptionStatus) -> AccountState {
        AccountState {
            plan,
            status,
            trial: false,
            current_period_end: None,
        }
    }

    // --- ISC-37-1: an active paid account is granted the Notes paid key ---
    #[test]
    fn active_account_is_granted_publish_note() {
        let ent = resolve_product_entitlements(
            "acct_pro",
            &state(PlanTier::Pro, SubscriptionStatus::Active),
            NOW,
        );
        assert!(
            ent.allows(&publish_note_key()),
            "an active paid account may publish a note"
        );
    }

    // --- ISC-37-2: a free account is denied the Notes paid key ---
    #[test]
    fn free_account_is_denied_publish_note() {
        let ent = resolve_product_entitlements(
            "acct_free",
            &state(PlanTier::Free, SubscriptionStatus::Free),
            NOW,
        );
        assert!(
            !ent.allows(&publish_note_key()),
            "a free account may not publish a note"
        );
    }

    // --- ISC-37-1b: a trial account is granted the Notes paid key ---
    #[test]
    fn trial_account_is_granted_publish_note() {
        let mut s = state(PlanTier::Pro, SubscriptionStatus::Trialing);
        s.trial = true;
        let ent = resolve_product_entitlements("acct_trial", &s, NOW);
        assert!(
            ent.allows(&publish_note_key()),
            "a trial account may publish a note"
        );
    }

    // --- ISC-37-2b: a past-due account is denied the Notes paid key ---
    #[test]
    fn past_due_account_is_denied_publish_note() {
        let ent = resolve_product_entitlements(
            "acct_pd",
            &state(PlanTier::Pro, SubscriptionStatus::PastDue),
            NOW,
        );
        assert!(
            !ent.allows(&publish_note_key()),
            "a past-due account may not publish a note"
        );
    }

    // --- ISC-37-2c: canceled access ends at the period boundary, mirroring the
    // spine engine's inclusive-of-loss boundary (`now >= end` denies). ---
    #[test]
    fn canceled_account_grants_until_period_end_then_denies() {
        let canceled = |end: u64| AccountState {
            plan: PlanTier::Pro,
            status: SubscriptionStatus::Canceled,
            trial: false,
            current_period_end: Some(end),
        };
        // One second before the boundary: still granted.
        assert!(
            resolve_product_entitlements("a", &canceled(NOW + 1), NOW).allows(&publish_note_key())
        );
        // Exactly at the boundary: access has ended.
        assert!(!resolve_product_entitlements("a", &canceled(NOW), NOW).allows(&publish_note_key()));
        // After the boundary: ended.
        assert!(
            !resolve_product_entitlements("a", &canceled(NOW - 1), NOW).allows(&publish_note_key())
        );
    }
}
