import { useState } from "react";
import { BillingRequestError, openPortal, startCheckout } from "./billing";

/**
 * Billing affordances: "Upgrade" starts a checkout session and "Manage billing"
 * opens the customer portal — both by asking the cloud backend for a URL and
 * opening it in the system browser (issue #31 acceptance criterion 3).
 *
 * Authority boundary (ADR-0001): this component decides nothing about payment. It
 * calls the backend, which is the billing authority, and opens the URL the backend
 * returned via the default {@link startCheckout}/{@link openPortal} opener (the
 * system browser). It holds no Stripe secret.
 */

const DEFAULT_API_BASE_URL = "http://localhost:8787";

export interface BillingPanelProps {
  /** Bearer token for the authenticated request. */
  token: string;
  /** Cloud API base URL. Defaults to the local dev backend. */
  baseUrl?: string;
}

type ActionState =
  | { kind: "idle" }
  | { kind: "working" }
  | { kind: "error"; message: string };

export function BillingPanel({ token, baseUrl = DEFAULT_API_BASE_URL }: BillingPanelProps) {
  const [state, setState] = useState<ActionState>({ kind: "idle" });

  async function run(action: () => Promise<string>): Promise<void> {
    setState({ kind: "working" });
    try {
      await action();
      setState({ kind: "idle" });
    } catch (err: unknown) {
      setState({ kind: "error", message: messageFor(err) });
    }
  }

  return (
    <section aria-label="Billing">
      <button
        type="button"
        disabled={state.kind === "working"}
        onClick={() => void run(() => startCheckout(baseUrl, token, "pro"))}
      >
        Upgrade to Pro
      </button>
      <button
        type="button"
        disabled={state.kind === "working"}
        onClick={() => void run(() => openPortal(baseUrl, token))}
      >
        Manage billing
      </button>
      {state.kind === "error" && (
        <p role="alert">Could not open billing: {state.message}</p>
      )}
    </section>
  );
}

function messageFor(err: unknown): string {
  if (err instanceof BillingRequestError) return err.message;
  if (err instanceof Error) return err.message;
  return String(err);
}
