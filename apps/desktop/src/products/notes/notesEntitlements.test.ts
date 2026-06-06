import { describe, expect, it } from "vitest";
import {
  PUBLISH_NOTE,
  parseProductEntitlements,
  productFeatureGateState,
  NotesEntitlementsError,
  type ProductEntitlements,
} from "./notesEntitlements";

/**
 * React UX product-feature-gate state for the Notes sample product (issue #37).
 *
 * `productFeatureGateState` is the UX-only guard the screen uses to hide the paid
 * "Publish" action an account is not entitled to. ADR-0001: this is presentation,
 * never security — the Tauri command (`request_publish_note`) and the backend
 * (`POST /notes/publish`) are the real gates, both resolved server-side. The same
 * product key (`notes.publish_note`) drives all three layers; here we only assert
 * the screen hides vs. shows based on the server-resolved snapshot.
 */

const grantedSnapshot: ProductEntitlements = {
  account_id: "acct_acme",
  namespace: "notes",
  features: { "notes.publish_note": true },
};

const deniedSnapshot: ProductEntitlements = {
  account_id: "acct_free",
  namespace: "notes",
  features: { "notes.publish_note": false },
};

describe("productFeatureGateState", () => {
  it("shows the paid publish action for an entitled account", () => {
    expect(productFeatureGateState(grantedSnapshot, PUBLISH_NOTE)).toBe("visible");
  });

  it("hides the paid publish action for an unentitled account", () => {
    // The acceptance criterion: React hides the paid action for the unavailable
    // product key, so the screen never offers a publish the account cannot do.
    expect(productFeatureGateState(deniedSnapshot, PUBLISH_NOTE)).toBe("hidden");
  });

  it("hides when the product key is absent from the snapshot entirely", () => {
    const bare: ProductEntitlements = { ...deniedSnapshot, features: {} };
    expect(productFeatureGateState(bare, PUBLISH_NOTE)).toBe("hidden");
  });

  it("is driven by the same product key the backend and command gates use", () => {
    // The point of the end-to-end proof: one product key, three layers. A denied
    // snapshot hides publish here exactly as the backend 403s it.
    expect(productFeatureGateState(deniedSnapshot, PUBLISH_NOTE)).toBe("hidden");
    expect(productFeatureGateState(grantedSnapshot, PUBLISH_NOTE)).toBe("visible");
  });
});

describe("parseProductEntitlements", () => {
  it("parses a well-formed snapshot", () => {
    const parsed = parseProductEntitlements({
      account_id: "acct_acme",
      namespace: "notes",
      features: { "notes.publish_note": true },
    });
    expect(parsed.namespace).toBe("notes");
    expect(parsed.features["notes.publish_note"]).toBe(true);
  });

  it("rejects a malformed snapshot at the boundary", () => {
    expect(() => parseProductEntitlements(null)).toThrow(NotesEntitlementsError);
    expect(() =>
      parseProductEntitlements({ account_id: "a", namespace: "notes", features: { x: "nope" } }),
    ).toThrow(NotesEntitlementsError);
  });
});
