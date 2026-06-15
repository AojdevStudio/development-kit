import { MePanel } from "../Me";
import { BillingPanel } from "../BillingPanel";

/**
 * The kit's minimal platform-chrome slot for single-product mode (issue #66).
 *
 * A single-product app gives its product the root surface, but a real SaaS app
 * still needs account/subscription affordances reachable. This slot is that
 * affordance: it composes the existing, authority-backed `MePanel` (sign-in /
 * account state) and `BillingPanel` (Upgrade / Manage billing) so a single-product
 * app reaches account + billing WITHOUT forking the foundation or duplicating
 * billing logic. The panels already call the cloud authority (ADR-0001); this slot
 * only places them — it invents no billing UI of its own.
 *
 * Deliberately account + billing ONLY: `AdvancedReportPanel` is a product reporting
 * feature, not platform chrome, so it is not surfaced here. The slot is opt-in
 * (`ShellConfig.showPlatformChrome`) and renders alongside — not instead of — the
 * product root, so the product still owns the primary surface.
 */
interface PlatformChromeSlotProps {
  /** Bearer token forwarded to the authority-backed account/billing panels. */
  token: string;
}

export function PlatformChromeSlot({ token }: PlatformChromeSlotProps) {
  return (
    <aside aria-label="Account and billing">
      <MePanel token={token} />
      <BillingPanel token={token} />
    </aside>
  );
}
