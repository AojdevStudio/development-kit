/**
 * Shell configuration — the single place a consumer of the kit chooses the
 * desktop shell's mode (issue #63).
 *
 * Leave `primaryProduct` unset for a multi-product app (the default): the shell
 * renders generic chrome (app header + product nav + platform panels) framing the
 * registered products.
 *
 * Set `primaryProduct` to a registered product's `namespace` for a single-product
 * app — the shell then boots straight into that product's root screen, hiding the
 * generic product nav AND the platform chrome (the account/billing panels). A
 * sole-product app (e.g. OrinSync) IS the app, so its product owns the root surface:
 *
 *   export const shellConfig: ShellConfig = { primaryProduct: "orinsync" };
 *
 * By default a single-product app surfaces no kit billing/account UI — the product
 * owns the root outright. But a real single-product SaaS app still needs account
 * and subscription affordances (sign-in state, Upgrade, Manage billing) reachable
 * somewhere. Set `showPlatformChrome: true` to opt the app into the kit's minimal
 * platform-chrome slot (issue #66): the shell renders the existing, authority-backed
 * account/billing panels (`MePanel` + `BillingPanel`) alongside the product root,
 * so billing is reachable without forking the foundation or duplicating billing
 * logic. This reuses existing panels — it does not invent any billing UI.
 *
 *   export const shellConfig: ShellConfig = { primaryProduct: "orinsync", showPlatformChrome: true };
 *
 * This is pure configuration + presentation. The product-module seam
 * (BackendModule, LocalModule, ProductFeatureKey, products/<ns>/ screens) is
 * unchanged in either mode.
 */
export interface ShellConfig {
  /**
   * The `namespace` of the product that owns the root surface. When set, the
   * shell runs in single-product mode. Unset → multi-product mode. Must name a
   * product registered in `productRegistry`, or the shell fails fast at boot.
   */
  primaryProduct?: string;
  /**
   * Opt a single-product app into the kit's minimal platform-chrome slot
   * (account + billing affordances). Only consulted in single-product mode —
   * multi-product mode always renders platform chrome. Defaults to `false`, so a
   * single-product app owns the root completely unless it opts in (issue #66).
   */
  showPlatformChrome?: boolean;
}

/** The kit's default shell config: multi-product (no designated primary product). */
export const shellConfig: ShellConfig = {
  // Single-product apps set this to their product's namespace, e.g.:
  // primaryProduct: "orinsync",
  // and opt into the kit's account/billing slot with `showPlatformChrome: true`.
};
