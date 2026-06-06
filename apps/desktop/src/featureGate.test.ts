import { describe, expect, it } from "vitest";
import { featureGateState, type Entitlements } from "./entitlements";

/**
 * React UX feature-gate state (issue #30, acceptance criterion 1).
 *
 * `featureGateState` is the UX-only guard the screen uses to hide a paid feature
 * the account is not entitled to. ADR-0001: this is presentation, never security
 * — the Tauri command and the backend are the real gates. Same feature key
 * (`advanced_reports`) drives all three layers; here we only assert the screen
 * hides vs. shows based on the server-resolved snapshot.
 */

const proSnapshot: Entitlements = {
  account_id: "acct_acme",
  plan: "pro",
  status: "active",
  trial: false,
  features: { advanced_reports: true, max_projects: 100 },
};

const freeSnapshot: Entitlements = {
  account_id: "acct_free",
  plan: "free",
  status: "free",
  trial: false,
  features: { advanced_reports: false, max_projects: 3 },
};

describe("featureGateState", () => {
  it("shows the gated UI as visible for an entitled (Pro) account", () => {
    expect(featureGateState(proSnapshot, "advanced_reports")).toBe("visible");
  });

  it("hides the gated UI for a free / unentitled account", () => {
    // The acceptance criterion: React shows gated (hidden) UI for the
    // unavailable feature, so the screen never offers a paid action the account
    // cannot perform.
    expect(featureGateState(freeSnapshot, "advanced_reports")).toBe("hidden");
  });

  it("hides when the feature key is absent from the snapshot entirely", () => {
    const bare: Entitlements = { ...freeSnapshot, features: {} };
    expect(featureGateState(bare, "advanced_reports")).toBe("hidden");
  });

  it("treats a non-zero limit feature as visible (mirrors allows semantics)", () => {
    expect(featureGateState(proSnapshot, "max_projects")).toBe("visible");
    expect(featureGateState(freeSnapshot, "max_projects")).toBe("visible");
  });

  it("is driven by the same feature key the backend and command gates use", () => {
    // The point of the end-to-end proof: one feature key, three layers. A free
    // snapshot hides advanced_reports here exactly as the backend 403s it.
    expect(featureGateState(freeSnapshot, "advanced_reports")).toBe("hidden");
    expect(featureGateState(proSnapshot, "advanced_reports")).toBe("visible");
  });
});
