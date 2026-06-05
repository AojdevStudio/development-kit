import { useEffect, useState } from "react";
import { fetchMe, MeRequestError, type Me } from "./api";

/**
 * Loads and displays the current user and account by calling the cloud
 * authority's `GET /me` (issue #27 acceptance criterion 3).
 *
 * Authority boundary (ADR-0001): this component does not decide identity — it
 * reads what the backend resolved and renders it. Three observable states:
 * loading, error (including "not signed in"), and the resolved principal.
 */

/** Where the cloud API lives. Overridable so the same component works in dev,
 * packaged builds, and tests without code changes. */
const DEFAULT_API_BASE_URL = "http://localhost:8787";

export interface MePanelProps {
  /** Bearer token for the authenticated request. */
  token: string;
  /** Cloud API base URL. Defaults to the local dev backend. */
  baseUrl?: string;
}

type LoadState =
  | { kind: "loading" }
  | { kind: "error"; message: string; status?: number }
  | { kind: "loaded"; me: Me };

export function MePanel({ token, baseUrl = DEFAULT_API_BASE_URL }: MePanelProps) {
  const [state, setState] = useState<LoadState>({ kind: "loading" });

  useEffect(() => {
    let active = true;
    setState({ kind: "loading" });
    fetchMe(baseUrl, token)
      .then((me) => {
        if (active) setState({ kind: "loaded", me });
      })
      .catch((err: unknown) => {
        if (!active) return;
        const status = err instanceof MeRequestError ? err.status : undefined;
        setState({ kind: "error", message: messageFor(err), status });
      });
    return () => {
      active = false;
    };
  }, [token, baseUrl]);

  if (state.kind === "loading") {
    return <p aria-busy="true">Loading your account…</p>;
  }
  if (state.kind === "error") {
    const signedOut = state.status === 401;
    return (
      <p role="alert">
        {signedOut ? "You are not signed in." : `Could not load your account: ${state.message}`}
      </p>
    );
  }

  const { user, account, membership } = state.me;
  return (
    <section aria-label="Current account">
      <p>
        Signed in as <strong>{user.email}</strong>
      </p>
      <p>
        Account: <strong>{account.name}</strong> ({membership.role})
      </p>
    </section>
  );
}

function messageFor(err: unknown): string {
  if (err instanceof Error) return err.message;
  return String(err);
}
