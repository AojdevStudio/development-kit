import { describe, expect, it } from "vitest";
import { resolveShellLayout, ShellConfigError } from "./resolveShellLayout";
import type { DesktopProduct } from "./productRegistry";

/**
 * The shell's boot-time layout decision (issue #63). One config value
 * (`primaryProduct`) chooses between two modes:
 *
 *  - unset            → multi-product shell (generic chrome + product nav)
 *  - a product's `namespace` → single-product shell (that product owns the root)
 *
 * The decision is a pure function so both modes are verifiable here under
 * vitest-in-node (no DOM); the React `<Shell>` is a thin consumer of the result.
 *
 * Synthetic products keep this test pure (no React render, no Tauri import) and
 * prove the resolver is product-agnostic — it keys on `namespace`, never on a
 * specific product like `notes`, so OrinSync works through the same path.
 */

const Noop = () => null;
const orinsync: DesktopProduct = { namespace: "orinsync", title: "OrinSync", Root: Noop };
const notes: DesktopProduct = { namespace: "notes", title: "Notes", Root: Noop };
const registry: DesktopProduct[] = [orinsync, notes];

describe("resolveShellLayout", () => {
  it("renders the multi-product shell when no primary product is configured", () => {
    const layout = resolveShellLayout({}, registry);
    expect(layout.mode).toBe("multi-product");
    expect(layout.showProductNav).toBe(true);
    expect(layout.showPlatformChrome).toBe(true);
    expect(layout.primaryProduct).toBeNull();
    expect(layout.products).toEqual(registry);
  });

  it("boots one product into the root with no generic nav or chrome when primary is set", () => {
    const layout = resolveShellLayout({ primaryProduct: "orinsync" }, registry);
    expect(layout.mode).toBe("single-product");
    expect(layout.showProductNav).toBe(false);
    expect(layout.showPlatformChrome).toBe(false);
    expect(layout.primaryProduct).toBe(orinsync);
  });

  it("is product-agnostic — matches any registered namespace, not just notes", () => {
    expect(resolveShellLayout({ primaryProduct: "notes" }, registry).primaryProduct).toBe(notes);
    expect(resolveShellLayout({ primaryProduct: "orinsync" }, registry).primaryProduct).toBe(orinsync);
  });

  it("throws ShellConfigError when primaryProduct names an unregistered namespace", () => {
    expect(() => resolveShellLayout({ primaryProduct: "ghost" }, registry)).toThrow(ShellConfigError);
  });

  it("treats an empty-string primaryProduct as a misconfiguration, not as unset", () => {
    // The `=== undefined` guard deliberately does NOT treat "" as unset, so an
    // empty primaryProduct fails loudly rather than silently falling back to the
    // multi-product shell — "" is the most likely real misconfiguration.
    expect(() => resolveShellLayout({ primaryProduct: "" }, registry)).toThrow(ShellConfigError);
  });

  it("names the registered namespaces in the error so a misconfig is diagnosable", () => {
    expect(() => resolveShellLayout({ primaryProduct: "ghost" }, registry)).toThrow(
      /notes|orinsync/,
    );
  });

  // Issue #66: in single-product mode, `showPlatformChrome` is opt-in via config so
  // a single-product app can reach account/billing without forking the foundation.
  // The flag now DRIVES the kit's platform-chrome slot (MePanel + BillingPanel)
  // instead of being a dead seam.
  it("drives the platform-chrome slot in single-product mode when opted in via config", () => {
    const layout = resolveShellLayout(
      { primaryProduct: "orinsync", showPlatformChrome: true },
      registry,
    );
    expect(layout.mode).toBe("single-product");
    expect(layout.showPlatformChrome).toBe(true);
  });

  it("keeps the slot off by default in single-product mode (product owns the root)", () => {
    // #63's default is preserved: unless an app opts in, a single-product app shows
    // no kit chrome — the product IS the app.
    const layout = resolveShellLayout({ primaryProduct: "orinsync" }, registry);
    expect(layout.mode).toBe("single-product");
    expect(layout.showPlatformChrome).toBe(false);
  });

  it("honours an explicit showPlatformChrome:false in single-product mode", () => {
    const layout = resolveShellLayout(
      { primaryProduct: "orinsync", showPlatformChrome: false },
      registry,
    );
    expect(layout.showPlatformChrome).toBe(false);
  });

  it("ignores showPlatformChrome in multi-product mode (chrome is always on there)", () => {
    // The opt-in is a single-product concern; multi-product always renders chrome,
    // even if a config carries the field.
    const layout = resolveShellLayout({ showPlatformChrome: false }, registry);
    expect(layout.mode).toBe("multi-product");
    expect(layout.showPlatformChrome).toBe(true);
  });
});
