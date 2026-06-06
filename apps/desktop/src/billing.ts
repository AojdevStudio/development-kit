/**
 * Typed client for the cloud authority's billing surface.
 *
 * Authority boundary (ADR-0001): the desktop never decides payment state and
 * never talks to Stripe. It asks the cloud backend for a session URL
 * (`POST /billing/checkout`, `POST /billing/portal`) and opens that URL in the
 * system browser. It holds no Stripe secret and constructs no session URL of its
 * own — the URL is the backend's authoritative output (ADR-0002: no Stripe crate,
 * no billing key client-side).
 */

/** The plan tier a checkout targets, mirroring the backend `PlanTier` wire enum. */
export type PlanTier = "free" | "starter" | "pro" | "team" | "enterprise";

/** The `POST /billing/checkout` request body, mirroring `CheckoutSessionRequest`. */
export interface CheckoutSessionRequest {
  plan: PlanTier;
}

/** The error thrown when the backend rejects or fails a billing request. */
export class BillingRequestError extends Error {
  constructor(
    message: string,
    readonly status?: number,
  ) {
    super(message);
    this.name = "BillingRequestError";
  }
}

/**
 * Opens a URL in the user's system browser. Injectable so the same code is unit
 * testable with a fake opener and works in dev, packaged builds, and tests
 * without binding to a specific Tauri API import at module load.
 */
export type UrlOpener = (url: string) => Promise<void>;

/**
 * The default opener: hands the URL to the host to open in the system browser.
 *
 * Inside a Tauri v2 webview, an external `https://` URL opened with
 * `window.open(url, "_blank")` is routed to the OS default browser rather than a
 * new in-app window — which is exactly the billing requirement: checkout and the
 * customer portal must run in the real browser, never embedded (so the user sees
 * Stripe's real URL bar). This is the wired production default; tests inject a
 * fake opener instead. It pulls in no extra dependency and holds no Stripe secret.
 */
export const systemBrowserOpener: UrlOpener = async (url: string): Promise<void> => {
  if (typeof globalThis.window?.open === "function") {
    globalThis.window.open(url, "_blank");
    return;
  }
  // No window host available (non-browser context): surface rather than silently
  // drop, so a misconfigured caller is visible instead of a no-op "nothing opened".
  throw new BillingRequestError("no system browser available to open billing URL");
};

/**
 * Validate and narrow an unknown JSON value into a session URL. The desktop
 * trusts the backend for *authority* but still validates the *shape* at the
 * boundary, so a malformed response surfaces as a clear error rather than opening
 * `undefined` in the browser.
 */
function parseSessionUrl(value: unknown): string {
  if (typeof value !== "object" || value === null) {
    throw new BillingRequestError("malformed billing response: not an object");
  }
  const url = (value as Record<string, unknown>).url;
  if (typeof url !== "string" || url.length === 0) {
    throw new BillingRequestError("malformed billing response: missing url");
  }
  return url;
}

/**
 * Request a checkout session URL from the cloud backend for the chosen plan.
 *
 * Sends the bearer token in the `Authorization` header (the backend is the
 * authority; this is just transport). A non-2xx response — including 401 for an
 * unauthenticated request — is surfaced as a {@link BillingRequestError} carrying
 * the status. The account the session is bound to is resolved server-side from the
 * token; the desktop never sends an account id.
 */
export async function fetchCheckoutUrl(
  baseUrl: string,
  token: string,
  plan: PlanTier,
  fetchImpl: typeof fetch = fetch,
): Promise<string> {
  const body: CheckoutSessionRequest = { plan };
  const response = await fetchImpl(`${baseUrl}/billing/checkout`, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  });
  if (!response.ok) {
    throw new BillingRequestError("/billing/checkout request failed", response.status);
  }
  return parseSessionUrl(await response.json());
}

/**
 * Request a customer-portal session URL from the cloud backend.
 *
 * Same transport contract as {@link fetchCheckoutUrl}; no request body beyond the
 * bearer token, since the portal is bound to the authenticated account.
 */
export async function fetchPortalUrl(
  baseUrl: string,
  token: string,
  fetchImpl: typeof fetch = fetch,
): Promise<string> {
  const response = await fetchImpl(`${baseUrl}/billing/portal`, {
    method: "POST",
    headers: { Authorization: `Bearer ${token}` },
  });
  if (!response.ok) {
    throw new BillingRequestError("/billing/portal request failed", response.status);
  }
  return parseSessionUrl(await response.json());
}

/**
 * Start checkout: ask the backend for a checkout URL, then open it in the system
 * browser. This is the one call a "Upgrade" button makes. The URL is produced by
 * the backend and merely opened here — the desktop is not a billing authority.
 */
export async function startCheckout(
  baseUrl: string,
  token: string,
  plan: PlanTier,
  open: UrlOpener = systemBrowserOpener,
  fetchImpl: typeof fetch = fetch,
): Promise<string> {
  const url = await fetchCheckoutUrl(baseUrl, token, plan, fetchImpl);
  await open(url);
  return url;
}

/**
 * Open the billing portal: ask the backend for a portal URL, then open it in the
 * system browser. The one call a "Manage billing" button makes.
 */
export async function openPortal(
  baseUrl: string,
  token: string,
  open: UrlOpener = systemBrowserOpener,
  fetchImpl: typeof fetch = fetch,
): Promise<string> {
  const url = await fetchPortalUrl(baseUrl, token, fetchImpl);
  await open(url);
  return url;
}
