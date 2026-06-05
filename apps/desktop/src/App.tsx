import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

/**
 * Walking-skeleton shell. Proves the window opens and the React <-> Tauri IPC
 * round-trip works by calling the `ping` command. Product screens land in later
 * issues; this is intentionally minimal.
 */
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
    </main>
  );
}
