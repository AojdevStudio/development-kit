/**
 * Typed client and cache for the cloud authority's entitlements surface.
 *
 * Authority boundary (ADR-0001): the desktop never decides what paid access an
 * account has — it asks the cloud backend via `GET /me/entitlements` and reads
 * the answer. This module is the read-only seam: it issues the authenticated
 * request, parses the resolved entitlements, and caches the last good value so a
 * transient backend outage degrades to "known-good" rather than "no access".
 * It holds no secrets and makes no authorization decisions of its own.
 */

/** The plan tier an account is on, mirroring the backend `PlanTier` wire enum. */
export type PlanTier = "free" | "starter" | "pro" | "team" | "enterprise";

/** The subscription lifecycle state, mirroring the backend `SubscriptionStatus`. */
export type SubscriptionStatus =
  | "free"
  | "trialing"
  | "active"
  | "past_due"
  | "canceled"
  | "paused"
  | "incomplete";

/** A feature value: a boolean toggle or a numeric limit (mirrors `FeatureValue`). */
export type FeatureValue = boolean | number;

/**
 * A stable, explicit identifier for a gated capability — the baseline platform
 * keys, mirroring the backend `FeatureKey` enum. Typing `allows` against this
 * (rather than an arbitrary `string`) keeps the desktop's UX-gating callers from
 * drifting to typo'd or invented keys, matching the typed-form discipline the
 * Rust side enforces. Product modules extend the gated set via their own keys.
 */
export type FeatureKey =
  | "export_pdf"
  | "cloud_sync"
  | "advanced_reports"
  | "team_members"
  | "max_projects"
  | "priority_support"
  | "api_access";

/**
 * The account's computed paid access, mirroring the backend `Entitlements`. The
 * desktop reads `features` to drive UX-only gating; real enforcement is on the
 * server.
 */
export interface Entitlements {
  account_id: string;
  plan: PlanTier;
  status: SubscriptionStatus;
  trial: boolean;
  features: Record<string, FeatureValue>;
  license_expires_at?: number;
}

/** The `GET /me/entitlements` response, mirroring `EntitlementsResponse`. */
export interface EntitlementsResponse {
  entitlements: Entitlements;
}

/** The error thrown when the backend rejects or fails the entitlements request. */
export class EntitlementsRequestError extends Error {
  constructor(
    message: string,
    readonly status?: number,
  ) {
    super(message);
    this.name = "EntitlementsRequestError";
  }

  /**
   * Whether this error is an *authoritative denial* — the backend explicitly said
   * "no" about identity or access (401 unauthenticated, 403 forbidden, 404 no
   * account state). These are authority signals (ADR-0001), not transient
   * outages: the cache must NOT serve last-good entitlements over them, or a
   * revoked/canceled account would keep cached premium access.
   */
  get isAuthoritativeDenial(): boolean {
    return this.status === 401 || this.status === 403 || this.status === 404;
  }

  /**
   * Whether the cache may serve last-good entitlements over this error. ONLY a
   * genuinely transient failure qualifies: a thrown network error, or a 5xx /
   * other server status. An authoritative denial (401/403/404) and a *malformed
   * response* (a 2xx whose body failed validation — an `EntitlementsRequestError`
   * with no `status`) both mean the value on hand is not a trustworthy
   * authoritative answer, so neither may be papered over with stale access.
   */
  get isTransient(): boolean {
    return typeof this.status === "number" && this.status >= 500;
  }
}

const VALID_PLANS: ReadonlySet<string> = new Set([
  "free",
  "starter",
  "pro",
  "team",
  "enterprise",
]);
const VALID_STATUSES: ReadonlySet<string> = new Set([
  "free",
  "trialing",
  "active",
  "past_due",
  "canceled",
  "paused",
  "incomplete",
]);

/**
 * Validate and narrow an unknown JSON value into typed {@link Entitlements}. The
 * desktop trusts the backend for *authority* but still validates the *shape* at
 * the boundary, so a malformed response surfaces as a clear error rather than
 * leaking `undefined` into gating logic.
 */
export function parseEntitlements(value: unknown): Entitlements {
  const root = asObject(value, "response");
  const ent = asObject(root.entitlements, "entitlements");

  const plan = ent.plan;
  if (typeof plan !== "string" || !VALID_PLANS.has(plan)) {
    throw new EntitlementsRequestError(`malformed entitlements: unknown plan ${String(plan)}`);
  }
  const status = ent.status;
  if (typeof status !== "string" || !VALID_STATUSES.has(status)) {
    throw new EntitlementsRequestError(
      `malformed entitlements: unknown status ${String(status)}`,
    );
  }
  if (typeof ent.trial !== "boolean") {
    throw new EntitlementsRequestError("malformed entitlements: trial is not a boolean");
  }

  return {
    account_id: asString(ent.account_id, "account_id"),
    plan: plan as PlanTier,
    status: status as SubscriptionStatus,
    trial: ent.trial,
    features: parseFeatures(ent.features),
    ...(typeof ent.license_expires_at === "number"
      ? { license_expires_at: ent.license_expires_at }
      : {}),
  };
}

function parseFeatures(value: unknown): Record<string, FeatureValue> {
  if (value === undefined) return {};
  const obj = asObject(value, "features");
  const out: Record<string, FeatureValue> = {};
  for (const [key, raw] of Object.entries(obj)) {
    if (typeof raw === "boolean" || typeof raw === "number") {
      out[key] = raw;
    } else {
      throw new EntitlementsRequestError(`malformed entitlements: feature ${key} is not bool/number`);
    }
  }
  return out;
}

/**
 * Whether a feature key is allowed by these entitlements. Boolean features are
 * on/off; limit features are "allowed" when their ceiling is non-zero. This
 * mirrors the backend `Entitlements::allows`, but is UX-only — the server is the
 * real gate.
 */
export function allows(entitlements: Entitlements, feature: FeatureKey): boolean {
  const value = entitlements.features[feature];
  if (typeof value === "boolean") return value;
  if (typeof value === "number") return value > 0;
  return false;
}

/**
 * The visibility a paid feature's UI should have for these entitlements (issue
 * #30). `"visible"` when the server-resolved snapshot grants the feature,
 * `"hidden"` when it does not — the same feature key the Tauri command and the
 * backend gate use.
 *
 * Authority boundary (ADR-0001): this is UX gating ONLY. Hiding the control is a
 * convenience that keeps the screen honest; it is never the protection. A user
 * who reaches the action anyway is still refused by the Tauri command and the
 * backend, both of which decide from the same snapshot the backend computed. The
 * `entitlements` passed here must be a backend-fetched snapshot, never one the
 * desktop authored.
 */
export type GateVisibility = "visible" | "hidden";

export function featureGateState(
  entitlements: Entitlements,
  feature: FeatureKey,
): GateVisibility {
  return allows(entitlements, feature) ? "visible" : "hidden";
}

function asObject(value: unknown, field: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null) {
    throw new EntitlementsRequestError(`malformed entitlements: missing ${field}`);
  }
  return value as Record<string, unknown>;
}

function asString(value: unknown, field: string): string {
  if (typeof value !== "string") {
    throw new EntitlementsRequestError(`malformed entitlements: ${field} is not a string`);
  }
  return value;
}

/**
 * Fetch the account's entitlements from the cloud backend.
 *
 * Sends the bearer token in the `Authorization` header (the backend is the
 * authority; this is just transport). A non-2xx response — including 401 for an
 * unauthenticated request — is surfaced as an {@link EntitlementsRequestError}
 * carrying the status.
 */
export async function fetchEntitlements(
  baseUrl: string,
  token: string,
  fetchImpl: typeof fetch = fetch,
): Promise<Entitlements> {
  const response = await fetchImpl(`${baseUrl}/me/entitlements`, {
    headers: { Authorization: `Bearer ${token}` },
  });
  if (!response.ok) {
    throw new EntitlementsRequestError(`/me/entitlements request failed`, response.status);
  }
  return parseEntitlements(await response.json());
}

/**
 * A last-good cache for entitlements.
 *
 * Entitlements are the authoritative word from the backend, but the desktop must
 * stay usable across a *transient* outage (offline, 5xx, timeout). {@link load}
 * returns fresh entitlements when the fetch succeeds and stores them; on a
 * transient failure it serves the last cached value if one exists.
 *
 * Authority boundary (ADR-0001): an *authoritative denial* — 401/403/404, the
 * backend explicitly saying "no" about identity or access — is NOT a transient
 * failure. The cache never serves stale entitlements over a denial; it clears the
 * cached value and re-throws, so a revoked, canceled, or suspended account can
 * never keep cached premium access. Replaying a server-computed value across a
 * network blip is reading; serving it across a revocation would be the desktop
 * deciding access — which it must never do.
 */
export class EntitlementsCache {
  private last: Entitlements | undefined;

  /** The cached entitlements, or `undefined` if none have been stored yet. */
  get cached(): Entitlements | undefined {
    return this.last;
  }

  /** Drop any cached entitlements (e.g. on sign-out or an authoritative denial). */
  clear(): void {
    this.last = undefined;
  }

  /**
   * Fetch fresh entitlements, caching them on success.
   *
   * - Success → cache and return the fresh value.
   * - Authoritative denial (401/403/404) → clear the cache and re-throw; never
   *   serve stale access over a backend "no".
   * - Malformed response (a 2xx body that failed validation) → re-throw without
   *   serving cache; the value is not a trustworthy authoritative answer.
   * - Transient failure ONLY (network throw / 5xx) → serve the last cached value
   *   if one exists; otherwise re-throw so a cold-start failure is never swallowed.
   *
   * The cache is populated *only* here, from a successful backend fetch — there is
   * no public setter, so desktop code can never seed locally constructed
   * entitlements into the gate (ADR-0001: the desktop reads, never decides).
   */
  async load(
    baseUrl: string,
    token: string,
    fetchImpl: typeof fetch = fetch,
  ): Promise<Entitlements> {
    try {
      const fresh = await fetchEntitlements(baseUrl, token, fetchImpl);
      this.last = fresh;
      return fresh;
    } catch (err) {
      if (err instanceof EntitlementsRequestError && err.isAuthoritativeDenial) {
        // The backend said "no": stale access must not survive a revocation.
        this.last = undefined;
        throw err;
      }
      // Only a genuinely transient failure may fall back to last-good. A network
      // throw (not an EntitlementsRequestError) counts; a malformed 2xx body
      // (EntitlementsRequestError with no status) does not.
      const transient =
        !(err instanceof EntitlementsRequestError) || err.isTransient;
      if (transient && this.last !== undefined) return this.last;
      throw err;
    }
  }
}
