import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { MePanel } from "../Me";
import { BillingPanel } from "../BillingPanel";
import { AdvancedReportPanel } from "../AdvancedReportPanel";
import { PlatformChromeSlot } from "./PlatformChromeSlot";
import type { ShellLayout } from "./resolveShellLayout";

/**
 * The desktop shell (issue #63). Renders one of two layouts from a pre-resolved
 * `ShellLayout` (see `resolveShellLayout`):
 *
 *  - single-product → the primary product's root screen owns the surface: no
 *    generic product nav. Platform chrome is opt-in (issue #66): when
 *    `showPlatformChrome` is set, the kit's minimal account/billing slot
 *    (`PlatformChromeSlot`) renders alongside the product root so billing stays
 *    reachable; otherwise the product IS the app.
 *  - multi-product  → generic chrome (app header + platform panels) plus a product
 *    nav framing the active product's screen.
 *
 * `<Shell>` is a thin consumer; the mode decision lives in the pure
 * `resolveShellLayout` function, which is unit-tested for both modes (vitest runs
 * in node with no DOM, so the testable logic stays out of the component). The
 * component renders strictly from the descriptor's `showProductNav` /
 * `showPlatformChrome` flags — it never re-derives layout from `mode`.
 */

/** Dev seed token recognised by the walking-skeleton backend store (real auth lands later). */
const DEV_TOKEN = "tok_alice";

interface ShellProps {
  layout: ShellLayout;
}

export function Shell({ layout }: ShellProps) {
  if (layout.mode === "single-product" && layout.primaryProduct) {
    // Single-product app: the product owns the root surface. By default
    // (showPlatformChrome=false) that is all there is — no generic nav, no kit
    // wrapper, the product IS the app. When an app opts in (issue #66), the live
    // `showPlatformChrome` flag drives the kit's minimal account/billing slot
    // (`PlatformChromeSlot` = MePanel + BillingPanel) alongside the product root,
    // so billing stays reachable. The slot only places existing authority-backed
    // panels — it does NOT invent billing UI.
    const Root = layout.primaryProduct.Root;
    return (
      <>
        <Root />
        {layout.showPlatformChrome && <PlatformChromeSlot token={DEV_TOKEN} />}
      </>
    );
  }
  return <MultiProductShell layout={layout} />;
}

/** The generic multi-product shell: app header + product nav + platform panels. */
function MultiProductShell({ layout }: ShellProps) {
  const [pong, setPong] = useState<string>("…");
  const [activeNamespace, setActiveNamespace] = useState<string>(
    layout.products[0]?.namespace ?? "",
  );

  useEffect(() => {
    invoke<string>("ping")
      .then(setPong)
      .catch((err: unknown) => setPong(`error: ${String(err)}`));
  }, []);

  const active =
    layout.products.find((p) => p.namespace === activeNamespace) ?? layout.products[0];
  const ActiveRoot = active?.Root;

  return (
    <main style={{ fontFamily: "system-ui", padding: "2rem" }}>
      {layout.showPlatformChrome && (
        <>
          <h1>Development Kit</h1>
          <p>Tauri desktop SaaS starter — walking skeleton.</p>
          <p>
            Tauri command says: <strong>{pong}</strong>
          </p>
        </>
      )}
      {layout.showProductNav && (
        <nav aria-label="Products">
          {layout.products.map((product) => (
            <button
              key={product.namespace}
              type="button"
              aria-current={product.namespace === active?.namespace ? "page" : undefined}
              onClick={() => setActiveNamespace(product.namespace)}
            >
              {product.title}
            </button>
          ))}
        </nav>
      )}
      {layout.showPlatformChrome && (
        <>
          <MePanel token={DEV_TOKEN} />
          <BillingPanel token={DEV_TOKEN} />
          <AdvancedReportPanel token={DEV_TOKEN} />
        </>
      )}
      {ActiveRoot && <ActiveRoot />}
    </main>
  );
}
