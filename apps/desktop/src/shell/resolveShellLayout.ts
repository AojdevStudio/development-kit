import type { ShellConfig } from "./config";
import type { DesktopProduct } from "./productRegistry";

/** The shell's two layouts (issue #63). */
export type ShellMode = "single-product" | "multi-product";

/** The boot-time layout decision the React `<Shell>` renders. */
export interface ShellLayout {
  mode: ShellMode;
  /** Products available to the shell (the nav list in multi-product mode). */
  products: DesktopProduct[];
  /** The product that owns the root in single-product mode; null in multi-product. */
  primaryProduct: DesktopProduct | null;
  /** Whether the generic product nav renders. */
  showProductNav: boolean;
  /** Whether the generic platform chrome (header + account/billing panels) renders. */
  showPlatformChrome: boolean;
}

/** Thrown when `primaryProduct` names a product that is not registered. */
export class ShellConfigError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ShellConfigError";
  }
}

/**
 * Decide the shell layout from config + the registered products.
 *
 * - `primaryProduct` unset → multi-product shell: generic chrome + product nav.
 * - `primaryProduct` set to a registered `namespace` → single-product shell: that
 *   product owns the root; no generic nav or chrome.
 * - `primaryProduct` set to an unregistered namespace → `ShellConfigError` (fail
 *   fast; never a silent fallback or a blank root).
 *
 * Product-agnostic: it matches on `namespace`, so any product (notes, orinsync, …)
 * can be primary without touching this resolver.
 */
export function resolveShellLayout(
  config: ShellConfig,
  products: DesktopProduct[],
): ShellLayout {
  const primaryNamespace = config.primaryProduct;

  if (primaryNamespace === undefined) {
    return {
      mode: "multi-product",
      products,
      primaryProduct: null,
      showProductNav: true,
      showPlatformChrome: true,
    };
  }

  const primaryProduct = products.find((p) => p.namespace === primaryNamespace);
  if (primaryProduct === undefined) {
    const registered = products.map((p) => p.namespace).join(", ") || "(none)";
    throw new ShellConfigError(
      `shell.primaryProduct "${primaryNamespace}" is not a registered product; ` +
        `registered namespaces: ${registered}`,
    );
  }

  return {
    mode: "single-product",
    products,
    primaryProduct,
    showProductNav: false,
    showPlatformChrome: false,
  };
}
