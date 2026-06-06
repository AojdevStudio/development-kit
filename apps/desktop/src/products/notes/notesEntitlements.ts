/**
 * Typed client and UX-gate for the Notes product's entitlements (issue #37).
 *
 * Mirrors the spine's `entitlements.ts` but for the product key-space: the Notes
 * product declares gated capabilities as `namespace.name` product keys
 * (`notes.publish_note`), carried in a `ProductEntitlements` snapshot parallel to
 * the baseline `Entitlements`.
 *
 * Authority boundary (ADR-0001): the desktop never decides what paid access an
 * account has — it reads the snapshot the cloud backend computed. This module is
 * read-only: it validates the snapshot shape and exposes a UX-only gate. Real
 * enforcement is the Tauri command (`request_publish_note`) and the backend route
 * (`POST /notes/publish`), both of which resolve the snapshot server-side. Hiding
 * a control in React is convenience, never security.
 */

/** A product feature value: a boolean toggle or a numeric limit (mirrors `FeatureValue`). */
export type FeatureValue = boolean | number;

/**
 * A Notes product feature key. The product key-space is disjoint from the baseline
 * (`export_pdf`, …): every product key carries exactly one `.`, so it can never be
 * confused with a baseline key. Notes has one paid key today.
 */
export type NotesFeatureKey = "notes.publish_note";

/** The one paid Notes capability — shared verbatim across React, command, backend. */
export const PUBLISH_NOTE: NotesFeatureKey = "notes.publish_note";

/**
 * The account's computed Notes product access, mirroring the backend
 * `ProductEntitlements`. The desktop reads `features` to drive UX-only gating;
 * real enforcement is on the server.
 */
export interface ProductEntitlements {
  account_id: string;
  namespace: string;
  features: Record<string, FeatureValue>;
}

/** The error thrown when a Notes product-entitlements payload is malformed. */
export class NotesEntitlementsError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "NotesEntitlementsError";
  }
}

/**
 * Validate and narrow an unknown JSON value into typed {@link ProductEntitlements}.
 * The desktop trusts the backend for *authority* but still validates the *shape*
 * at the boundary, so a malformed response surfaces as a clear error rather than
 * leaking `undefined` into gating logic.
 */
export function parseProductEntitlements(value: unknown): ProductEntitlements {
  const root = asObject(value, "product entitlements");
  return {
    account_id: asString(root.account_id, "account_id"),
    namespace: asString(root.namespace, "namespace"),
    features: parseFeatures(root.features),
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
      throw new NotesEntitlementsError(`malformed product entitlements: feature ${key} is not bool/number`);
    }
  }
  return out;
}

/**
 * Whether a Notes product key is allowed by these entitlements. Boolean features
 * are on/off; limit features are "allowed" when their ceiling is non-zero. Mirrors
 * the backend `ProductEntitlements::allows`, but UX-only — the server is the real
 * gate.
 */
export function allowsProduct(
  entitlements: ProductEntitlements,
  feature: NotesFeatureKey,
): boolean {
  const value = entitlements.features[feature];
  if (typeof value === "boolean") return value;
  if (typeof value === "number") return value > 0;
  return false;
}

/** The visibility a paid Notes control should have for these entitlements. */
export type GateVisibility = "visible" | "hidden";

/**
 * The visibility a paid Notes feature's UI should have (UX only, ADR-0001).
 * `"visible"` when the server-resolved snapshot grants the key, `"hidden"`
 * otherwise — the same product key the Tauri command and the backend gate use. A
 * user who reaches the action anyway is still refused by the command and backend.
 */
export function productFeatureGateState(
  entitlements: ProductEntitlements,
  feature: NotesFeatureKey,
): GateVisibility {
  return allowsProduct(entitlements, feature) ? "visible" : "hidden";
}

function asObject(value: unknown, field: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null) {
    throw new NotesEntitlementsError(`malformed product entitlements: missing ${field}`);
  }
  return value as Record<string, unknown>;
}

function asString(value: unknown, field: string): string {
  if (typeof value !== "string") {
    throw new NotesEntitlementsError(`malformed product entitlements: ${field} is not a string`);
  }
  return value;
}
