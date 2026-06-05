import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { MePanel } from "./Me";

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
    </main>
  );
}
