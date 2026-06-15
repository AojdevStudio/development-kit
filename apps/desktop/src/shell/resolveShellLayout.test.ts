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

  it("fails loudly on misconfiguration — never silently falls back to multi-product", () => {
    // Anti-criterion: an unknown primary product must error, not render a blank
    // root or quietly behave like the multi-product shell.
    let threw = false;
    try {
      resolveShellLayout({ primaryProduct: "ghost" }, registry);
    } catch {
      threw = true;
    }
    expect(threw).toBe(true);
  });
});
