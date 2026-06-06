import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  EntitlementsCache,
  featureGateState,
  type Entitlements,
} from "./entitlements";

/**
 * The end-to-end worked example for issue #30: one concrete paid feature
 * (`advanced_reports`) gated across all three layers from the SAME server-resolved
 * entitlement snapshot.
 *
 * 1. React (this component) hides the "Generate advanced report" control when the
 *    snapshot does not grant the feature — UX only (ADR-0001).
 * 2. The Tauri command `request_advanced_report` refuses the local action without
 *    the entitlement — the local guard.
 * 3. The backend `POST /gated-feature/advanced_reports` is the authority.
 *
 * The snapshot is FETCHED from the cloud authority (`GET /me/entitlements`) via
 * {@link EntitlementsCache}; this component never constructs entitlements of its
 * own. The desktop reads access, it never decides it.
 */

const DEFAULT_API_BASE_URL = "http://localhost:8787";

/** The feature key this panel gates — shared verbatim across all three layers. */
const ADVANCED_REPORTS = "advanced_reports" as const;

export interface AdvancedReportPanelProps {
  /** Bearer token for the authenticated entitlements request. */
  token: string;
  /** Cloud API base URL. Defaults to the local dev backend. */
  baseUrl?: string;
  /** Injectable cache so tests can drive a known snapshot. Defaults to a fresh one. */
  cache?: EntitlementsCache;
  /** Injectable command invoker so tests can assert the allow path without Tauri. */
  invokeCommand?: typeof invoke;
}

type PanelState =
  | { kind: "loading" }
  | { kind: "error"; message: string }
  | { kind: "ready"; entitlements: Entitlements };

export function AdvancedReportPanel({
  token,
  baseUrl = DEFAULT_API_BASE_URL,
  cache = new EntitlementsCache(),
  invokeCommand = invoke,
}: AdvancedReportPanelProps) {
  const [state, setState] = useState<PanelState>({ kind: "loading" });
  const [report, setReport] = useState<string | null>(null);
  const [denied, setDenied] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    setState({ kind: "loading" });
    cache
      .load(baseUrl, token)
      .then((entitlements) => {
        if (active) setState({ kind: "ready", entitlements });
      })
      .catch((err: unknown) => {
        if (active)
          setState({ kind: "error", message: err instanceof Error ? err.message : String(err) });
      });
    return () => {
      active = false;
    };
  }, [token, baseUrl, cache]);

  if (state.kind === "loading") {
    return <p aria-busy="true">Loading feature access…</p>;
  }
  if (state.kind === "error") {
    return <p role="alert">Could not load feature access: {state.message}</p>;
  }

  // UX gate (ADR-0001: presentation only). The Tauri command and backend are the
  // real gates; hiding the control just keeps the screen honest.
  const visibility = featureGateState(state.entitlements, ADVANCED_REPORTS);
  if (visibility === "hidden") {
    return (
      <section aria-label="Advanced reports">
        <p>Advanced reports are a paid feature. Upgrade to enable them.</p>
      </section>
    );
  }

  const generate = () => {
    setDenied(null);
    // Pass the server-fetched snapshot to the local command gate; the command
    // refuses if the snapshot does not grant the feature.
    invokeCommand<string>("request_advanced_report", { entitlements: state.entitlements })
      .then((payload) => setReport(payload))
      .catch((err: unknown) => setDenied(err instanceof Error ? err.message : String(err)));
  };

  return (
    <section aria-label="Advanced reports">
      <button type="button" onClick={generate}>
        Generate advanced report
      </button>
      {report && <p>Report: {report}</p>}
      {denied && <p role="alert">Action refused: {denied}</p>}
    </section>
  );
}
