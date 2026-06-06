//! The entitlement engine: resolve an account's billing state into the concrete
//! [`Entitlements`] it is granted (issue #29).
//!
//! This is where paid access is *decided*, always on the server (ADR-0001). The
//! engine is a pure, total function over a typed [`AccountState`] — plan,
//! subscription status, trial, and the period boundary that governs
//! cancel/expiry — so every acceptance scenario (trial, active, past-due,
//! canceled, downgrade) is unit-testable without a router and a downgrade needs
//! no special branch (it reads the *current* plan). The desktop never runs this;
//! it reads the result via `GET /me/entitlements`.
//!
//! Account state enters the system through [`AccountStateStore`], mirroring the
//! [`crate::auth::PrincipalStore`] seam from #27: an in-memory dev store now, a
//! Postgres-backed store later, with nothing above the trait changing.

use std::collections::BTreeMap;

use shared::{Entitlements, FeatureKey, FeatureValue, PlanTier, SubscriptionStatus};

/// The "effectively unlimited" ceiling for tiers with no practical cap
/// (Enterprise). Chosen to stay within JavaScript's safe-integer range
/// (`Number.MAX_SAFE_INTEGER` = 2^53 − 1) so the desktop, which parses limits as
/// JS numbers, can represent it without precision loss. A real cap no account
/// will hit, not a magic `u64::MAX` that would overflow on the wire.
pub const UNLIMITED: u64 = 9_007_199_254_740_991; // 2^53 - 1

/// The account's current billing state — the typed input to the engine.
///
/// Held as closed enums plus a period boundary so impossible states are
/// unrepresentable and the engine match is total. `current_period_end` is the
/// unix-epoch-seconds instant a canceled (or lapsed) subscription stops granting
/// access; `None` means "no bounded period" (e.g. an active sub or a free plan).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountState {
    /// The plan tier the account is currently on. A *downgrade* is simply a
    /// lower tier here — the engine reads the current plan, so no special case.
    pub plan: PlanTier,
    /// The subscription lifecycle state, set by the backend from Stripe.
    pub status: SubscriptionStatus,
    /// Whether the account is inside a trial. Trial grants paid features.
    pub trial: bool,
    /// Unix epoch seconds when the current billing period ends. Governs whether a
    /// canceled subscription still grants access (`now < end`) or has lapsed.
    pub current_period_end: Option<u64>,
}

impl AccountState {
    /// Whether a canceled/lapsed subscription is still inside its paid period at
    /// `now`. Boundary matches [`shared::LicenseToken::is_expired_at`]: access
    /// ends *at* `current_period_end`, so `now < end` still grants.
    fn within_period(&self, now: u64) -> bool {
        match self.current_period_end {
            Some(end) => now < end,
            None => false,
        }
    }
}

/// Resolve an account's billing state into its [`Entitlements`] at instant `now`.
///
/// Pure and total. The policy: the plan tier sets the feature *ceiling*; the
/// subscription status decides whether that ceiling is granted in full, degraded,
/// or collapsed to free. `now` is injected (not read from the wall clock) so the
/// period-boundary scenarios are deterministic in tests.
pub fn resolve_entitlements(
    account_id: impl Into<String>,
    state: &AccountState,
    now: u64,
) -> Entitlements {
    use SubscriptionStatus::*;

    // The effective access decision, independent of which tier's features apply.
    let grant = match state.status {
        // Trial and active subscriptions grant the plan's full feature set.
        Trialing | Active => Access::Full,
        // Past-due is a grace period: keep the basics, drop premium sync/reporting.
        PastDue => Access::Degraded,
        // Canceled still grants until the period boundary, then collapses to free.
        Canceled => {
            if state.within_period(now) {
                Access::Full
            } else {
                Access::Free
            }
        }
        // Free / paused / incomplete carry no paid access.
        Free | Paused | Incomplete => Access::Free,
    };

    let features = match grant {
        Access::Full => plan_features(state.plan),
        Access::Degraded => degrade(plan_features(state.plan)),
        Access::Free => plan_features(PlanTier::Free),
    };

    Entitlements {
        account_id: account_id.into(),
        plan: state.plan,
        status: state.status,
        trial: state.trial,
        features,
        license_expires_at: None,
    }
}

/// The effective access level the subscription status grants over the plan's
/// feature ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Access {
    /// The plan's full feature set.
    Full,
    /// A degraded subset (past-due grace).
    Degraded,
    /// Free-tier features only.
    Free,
}

/// The full feature set a plan tier is entitled to when access is granted.
///
/// This is the single per-tier policy table — adding a [`FeatureKey`] or a tier
/// is a one-site edit here. Boolean keys are on/off; limit keys carry a ceiling
/// that scales with tier.
fn plan_features(plan: PlanTier) -> BTreeMap<FeatureKey, FeatureValue> {
    use FeatureKey::*;
    use FeatureValue::{Enabled, Limit};

    let mut f = BTreeMap::new();
    match plan {
        PlanTier::Free => {
            f.insert(ExportPdf, Enabled(false));
            f.insert(CloudSync, Enabled(false));
            f.insert(AdvancedReports, Enabled(false));
            f.insert(PrioritySupport, Enabled(false));
            f.insert(ApiAccess, Enabled(false));
            f.insert(MaxProjects, Limit(3));
            f.insert(TeamMembers, Limit(0));
        }
        PlanTier::Starter => {
            f.insert(ExportPdf, Enabled(true));
            f.insert(CloudSync, Enabled(true));
            f.insert(AdvancedReports, Enabled(false));
            f.insert(PrioritySupport, Enabled(false));
            f.insert(ApiAccess, Enabled(false));
            f.insert(MaxProjects, Limit(25));
            f.insert(TeamMembers, Limit(1));
        }
        PlanTier::Pro => {
            f.insert(ExportPdf, Enabled(true));
            f.insert(CloudSync, Enabled(true));
            f.insert(AdvancedReports, Enabled(true));
            f.insert(PrioritySupport, Enabled(true));
            f.insert(ApiAccess, Enabled(true));
            f.insert(MaxProjects, Limit(100));
            f.insert(TeamMembers, Limit(5));
        }
        PlanTier::Team => {
            f.insert(ExportPdf, Enabled(true));
            f.insert(CloudSync, Enabled(true));
            f.insert(AdvancedReports, Enabled(true));
            f.insert(PrioritySupport, Enabled(true));
            f.insert(ApiAccess, Enabled(true));
            f.insert(MaxProjects, Limit(1000));
            f.insert(TeamMembers, Limit(50));
        }
        PlanTier::Enterprise => {
            f.insert(ExportPdf, Enabled(true));
            f.insert(CloudSync, Enabled(true));
            f.insert(AdvancedReports, Enabled(true));
            f.insert(PrioritySupport, Enabled(true));
            f.insert(ApiAccess, Enabled(true));
            f.insert(MaxProjects, Limit(UNLIMITED));
            f.insert(TeamMembers, Limit(UNLIMITED));
        }
    }
    f
}

/// The past-due degrade policy: a grace window that keeps the basics (export,
/// project access) but revokes the premium sync/reporting/API surface. Encoded
/// once here so "what degrades" lives in exactly one place.
fn degrade(mut features: BTreeMap<FeatureKey, FeatureValue>) -> BTreeMap<FeatureKey, FeatureValue> {
    use FeatureKey::*;
    use FeatureValue::Enabled;
    for key in [CloudSync, AdvancedReports, ApiAccess, PrioritySupport] {
        features.insert(key, Enabled(false));
    }
    features
}

/// Resolve an `account_id` to its current [`AccountState`], or `None` if the
/// account has no billing state on record.
///
/// A small interface over a deep implementation (ADR-0002 spirit): callers ask
/// "what is this account's billing state?" and never reason about storage. The
/// walking-skeleton implementation is in-memory ([`InMemoryAccountStateStore`]);
/// the production implementation is Postgres-backed and lands in its own issue.
pub trait AccountStateStore: Send + Sync {
    /// The account's current billing state, or `None` if unknown.
    fn account_state(&self, account_id: &str) -> Option<AccountState>;
}

/// An in-memory [`AccountStateStore`] — the walking-skeleton billing-state
/// backing for `GET /me/entitlements`.
///
/// Mirrors [`crate::store::InMemoryPrincipalStore`]: a fixed map of account ids
/// to billing state so the entitlement path is real and end-to-end testable now,
/// before the durable Postgres-backed store exists. The swap changes nothing
/// above the [`AccountStateStore`] trait.
#[derive(Debug, Clone, Default)]
pub struct InMemoryAccountStateStore {
    by_account: std::collections::HashMap<String, AccountState>,
}

impl InMemoryAccountStateStore {
    /// An empty store. Every account resolves to `None`.
    pub fn new() -> Self {
        Self::default()
    }

    /// The walking-skeleton dev store: the dev account (`acct_acme`, the account
    /// the #27 dev principal belongs to) resolves to an active Pro subscription,
    /// so the desktop dev build can load real paid entitlements end-to-end.
    pub fn dev_seed() -> Self {
        Self::new().with_account(
            crate::store::dev_principal().account_id,
            dev_account_state(),
        )
    }

    /// Bind `account_id` to `state`, replacing any existing binding. Chainable.
    pub fn with_account(mut self, account_id: impl Into<String>, state: AccountState) -> Self {
        self.by_account.insert(account_id.into(), state);
        self
    }
}

/// The billing state the dev account resolves to in the walking skeleton: an
/// active Pro subscription with no bounded period.
pub fn dev_account_state() -> AccountState {
    AccountState {
        plan: PlanTier::Pro,
        status: SubscriptionStatus::Active,
        trial: false,
        current_period_end: None,
    }
}

impl AccountStateStore for InMemoryAccountStateStore {
    fn account_state(&self, account_id: &str) -> Option<AccountState> {
        self.by_account.get(account_id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed instant used by tests that don't care about the period boundary.
    const NOW: u64 = 1_700_000_000;

    fn state(plan: PlanTier, status: SubscriptionStatus) -> AccountState {
        AccountState {
            plan,
            status,
            trial: false,
            current_period_end: None,
        }
    }

    // --- ISC-4: trial ---
    #[test]
    fn trial_user_receives_trial_entitlements() {
        let mut s = state(PlanTier::Pro, SubscriptionStatus::Trialing);
        s.trial = true;
        let ent = resolve_entitlements("acct_trial", &s, NOW);
        assert!(ent.allows(FeatureKey::ExportPdf));
        assert!(ent.allows(FeatureKey::CloudSync));
        assert!(ent.trial);
    }

    // --- ISC-5: active paid ---
    #[test]
    fn active_paid_user_receives_paid_entitlements() {
        let ent = resolve_entitlements(
            "acct_pro",
            &state(PlanTier::Pro, SubscriptionStatus::Active),
            NOW,
        );
        assert!(ent.allows(FeatureKey::ExportPdf));
        assert!(ent.allows(FeatureKey::CloudSync));
        assert!(ent.allows(FeatureKey::AdvancedReports));
    }

    // --- ISC-6: past-due degrades ---
    #[test]
    fn past_due_degrades_per_policy() {
        let ent = resolve_entitlements(
            "acct_pd",
            &state(PlanTier::Pro, SubscriptionStatus::PastDue),
            NOW,
        );
        // Grace keeps the basics...
        assert!(ent.allows(FeatureKey::ExportPdf));
        assert!(ent.allows(FeatureKey::MaxProjects));
        // ...but drops premium sync/reporting/API.
        assert!(!ent.allows(FeatureKey::CloudSync));
        assert!(!ent.allows(FeatureKey::AdvancedReports));
        assert!(!ent.allows(FeatureKey::ApiAccess));
    }

    // --- ISC-7: canceled past period → free ---
    #[test]
    fn canceled_loses_access_at_period_end() {
        let s = AccountState {
            plan: PlanTier::Pro,
            status: SubscriptionStatus::Canceled,
            trial: false,
            current_period_end: Some(NOW), // period ends exactly at NOW
        };
        let ent = resolve_entitlements("acct_cx", &s, NOW);
        // At the boundary, access has ended → free features only.
        assert!(!ent.allows(FeatureKey::ExportPdf));
        assert!(!ent.allows(FeatureKey::CloudSync));
        assert!(!ent.allows(FeatureKey::AdvancedReports));
    }

    // --- ISC-7 boundary: access ends exactly AT current_period_end ---
    #[test]
    fn canceled_period_end_boundary_is_inclusive_of_loss() {
        // Pin the `now >= end => free` semantics so they can't silently regress
        // to `now > end` (which would grant a free extra second of paid access).
        let canceled = |end: u64| AccountState {
            plan: PlanTier::Pro,
            status: SubscriptionStatus::Canceled,
            trial: false,
            current_period_end: Some(end),
        };
        // One second before the boundary: still granted.
        assert!(resolve_entitlements("a", &canceled(NOW + 1), NOW).allows(FeatureKey::ExportPdf));
        // Exactly at the boundary: access has ended.
        assert!(!resolve_entitlements("a", &canceled(NOW), NOW).allows(FeatureKey::ExportPdf));
        // After the boundary: ended.
        assert!(!resolve_entitlements("a", &canceled(NOW - 1), NOW).allows(FeatureKey::ExportPdf));
    }

    // --- ISC-8: canceled within period → retains ---
    #[test]
    fn canceled_retains_access_until_period_end() {
        let s = AccountState {
            plan: PlanTier::Pro,
            status: SubscriptionStatus::Canceled,
            trial: false,
            current_period_end: Some(NOW + 86_400), // a day left
        };
        let ent = resolve_entitlements("acct_cx", &s, NOW);
        assert!(ent.allows(FeatureKey::ExportPdf));
        assert!(ent.allows(FeatureKey::CloudSync));
        assert!(ent.allows(FeatureKey::AdvancedReports));
    }

    // --- ISC-9: downgrade loses over-tier features ---
    #[test]
    fn downgraded_user_loses_over_tier_features() {
        // A Pro account downgraded to Starter: the engine reads the *current*
        // plan, so the Pro-only features are simply absent. No special branch.
        let ent = resolve_entitlements(
            "acct_dg",
            &state(PlanTier::Starter, SubscriptionStatus::Active),
            NOW,
        );
        assert!(ent.allows(FeatureKey::ExportPdf)); // Starter still has these
        assert!(ent.allows(FeatureKey::CloudSync));
        assert!(!ent.allows(FeatureKey::AdvancedReports)); // Pro-only
        assert!(!ent.allows(FeatureKey::ApiAccess)); // Pro-only
    }

    // --- ISC-10: free plan ---
    #[test]
    fn free_plan_resolves_to_no_paid_features() {
        let ent = resolve_entitlements(
            "acct_free",
            &state(PlanTier::Free, SubscriptionStatus::Free),
            NOW,
        );
        assert!(!ent.allows(FeatureKey::ExportPdf));
        assert!(!ent.allows(FeatureKey::CloudSync));
        assert!(!ent.allows(FeatureKey::AdvancedReports));
        assert!(!ent.allows(FeatureKey::ApiAccess));
    }

    // --- ISC-11: trial flag mirrors input ---
    #[test]
    fn entitlements_trial_flag_matches_state() {
        let mut s = state(PlanTier::Pro, SubscriptionStatus::Trialing);
        s.trial = true;
        assert!(resolve_entitlements("a", &s, NOW).trial);

        let s2 = state(PlanTier::Pro, SubscriptionStatus::Active);
        assert!(!resolve_entitlements("a", &s2, NOW).trial);
    }

    // --- ISC-12: plan/status carried through ---
    #[test]
    fn entitlements_carries_resolved_plan_and_status() {
        let ent =
            resolve_entitlements("a", &state(PlanTier::Team, SubscriptionStatus::Active), NOW);
        assert_eq!(ent.plan, PlanTier::Team);
        assert_eq!(ent.status, SubscriptionStatus::Active);
    }

    // --- ISC-13: tier limits scale ---
    #[test]
    fn tier_limits_scale_with_plan() {
        let free = resolve_entitlements("a", &state(PlanTier::Free, SubscriptionStatus::Free), NOW);
        let pro = resolve_entitlements("a", &state(PlanTier::Pro, SubscriptionStatus::Active), NOW);
        let free_max = match free.features.get(&FeatureKey::MaxProjects) {
            Some(FeatureValue::Limit(n)) => *n,
            _ => panic!("free max_projects should be a limit"),
        };
        let pro_max = match pro.features.get(&FeatureKey::MaxProjects) {
            Some(FeatureValue::Limit(n)) => *n,
            _ => panic!("pro max_projects should be a limit"),
        };
        assert!(pro_max > free_max, "pro tier raises the project ceiling");
    }

    // --- ISC-15: dev store seeds a known account ---
    #[test]
    fn dev_store_seeds_known_account() {
        let store = InMemoryAccountStateStore::dev_seed();
        let acct = crate::store::dev_principal().account_id;
        let state = store.account_state(&acct).expect("dev account is seeded");
        assert_eq!(state.plan, PlanTier::Pro);
        assert_eq!(state.status, SubscriptionStatus::Active);
    }

    // --- ISC-16: unknown account resolves to None ---
    #[test]
    fn unknown_account_resolves_none() {
        let store = InMemoryAccountStateStore::dev_seed();
        assert_eq!(store.account_state("acct_nope"), None);
    }
}
