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
 * generic product nav and platform chrome. A sole-product app (e.g. OrinSync) IS
 * the app, so its product owns the root surface:
 *
 *   export const shellConfig: ShellConfig = { primaryProduct: "orinsync" };
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
}

/** The kit's default shell config: multi-product (no designated primary product). */
export const shellConfig: ShellConfig = {
  // Single-product apps set this to their product's namespace, e.g.:
  // primaryProduct: "orinsync",
};
