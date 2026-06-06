import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { MePanel } from "./Me";
import { BillingPanel } from "./BillingPanel";
import { AdvancedReportPanel } from "./AdvancedReportPanel";
import { NotesScreen } from "./products/notes/NotesScreen";
import type { ProductEntitlements } from "./products/notes/notesEntitlements";

/**
 * Walking-skeleton shell. Proves the window opens and the React <-> Tauri IPC
 * round-trip works by calling the `ping` command, and loads the current
 * user/account from the cloud authority via `GET /me` (issue #27).
 *
 * The dev bearer token below is a placeholder for the real sign-in/session flow,
 * which lands in a later issue; the authority that resolves it always lives in
 * the cloud backend (ADR-0001).
 */

/** Dev seed token recognised by the walking-skeleton backend store. */
const DEV_TOKEN = "tok_alice";

/**
 * A deny-by-default Notes product snapshot. Until the desktop fetches the real
 * `ProductEntitlements` from the backend (the snapshot is authored server-side,
 * ADR-0001), the screen grants nothing — the safe direction the product-module
 * seam endorses. The Tauri command and backend route remain the real authority
 * regardless of what the screen shows.
 */
const EMPTY_NOTES_ENTITLEMENTS: ProductEntitlements = {
  account_id: "",
  namespace: "notes",
  features: {},
};

export function App() {
  const [pong, setPong] = useState<string>("…");

  useEffect(() => {
    invoke<string>("ping")
      .then(setPong)
      .catch((err: unknown) => setPong(`error: ${String(err)}`));
  }, []);

  return (
    <main style={{ fontFamily: "system-ui", padding: "2rem" }}>
      <h1>Development Kit</h1>
      <p>Tauri desktop SaaS starter — walking skeleton.</p>
      <p>
        Tauri command says: <strong>{pong}</strong>
      </p>
      <MePanel token={DEV_TOKEN} />
      <BillingPanel token={DEV_TOKEN} />
      <AdvancedReportPanel token={DEV_TOKEN} />
      {/* The Notes sample product (issue #37), contributed via the seam. */}
      <NotesScreen entitlements={EMPTY_NOTES_ENTITLEMENTS} />
    </main>
  );
}
