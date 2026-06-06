import { describe, expect, it, vi } from "vitest";
import {
  BillingRequestError,
  fetchCheckoutUrl,
  fetchPortalUrl,
  openPortal,
  startCheckout,
  type UrlOpener,
} from "./billing";

const CHECKOUT_URL = "https://checkout.stripe.com/c/pay/mock_acct_acme_pro";
const PORTAL_URL = "https://billing.stripe.com/p/session/mock_acct_acme";

describe("fetchCheckoutUrl", () => {
  it("POSTs the chosen plan with the bearer token and returns the backend URL", async () => {
    let seenUrl = "";
    let seenMethod = "";
    let seenAuth: string | null = null;
    let seenBody = "";
    const fakeFetch = (async (url: string, init?: RequestInit) => {
      seenUrl = url;
      seenMethod = init?.method ?? "";
      seenAuth = new Headers(init?.headers).get("Authorization");
      seenBody = String(init?.body ?? "");
      return new Response(JSON.stringify({ url: CHECKOUT_URL }), { status: 200 });
    }) as unknown as typeof fetch;

    const url = await fetchCheckoutUrl("http://localhost:8787", "tok_alice", "pro", fakeFetch);

    expect(seenUrl).toBe("http://localhost:8787/billing/checkout");
    expect(seenMethod).toBe("POST");
    expect(seenAuth).toBe("Bearer tok_alice");
    expect(JSON.parse(seenBody)).toEqual({ plan: "pro" });
    expect(url).toBe(CHECKOUT_URL);
  });

  it("throws a BillingRequestError carrying the status on a 401", async () => {
    const fakeFetch = (async () =>
      new Response("", { status: 401 })) as unknown as typeof fetch;
    await expect(
      fetchCheckoutUrl("http://localhost:8787", "bad", "pro", fakeFetch),
    ).rejects.toMatchObject({ name: "BillingRequestError", status: 401 });
  });

  it("rejects a malformed 2xx body that has no url", async () => {
    const fakeFetch = (async () =>
      new Response(JSON.stringify({ nope: true }), { status: 200 })) as unknown as typeof fetch;
    await expect(
      fetchCheckoutUrl("http://localhost:8787", "tok_alice", "pro", fakeFetch),
    ).rejects.toThrow(BillingRequestError);
  });
});

describe("fetchPortalUrl", () => {
  it("POSTs with the bearer token and returns the backend portal URL", async () => {
    let seenUrl = "";
    let seenAuth: string | null = null;
    const fakeFetch = (async (url: string, init?: RequestInit) => {
      seenUrl = url;
      seenAuth = new Headers(init?.headers).get("Authorization");
      return new Response(JSON.stringify({ url: PORTAL_URL }), { status: 200 });
    }) as unknown as typeof fetch;

    const url = await fetchPortalUrl("http://localhost:8787", "tok_alice", fakeFetch);

    expect(seenUrl).toBe("http://localhost:8787/billing/portal");
    expect(seenAuth).toBe("Bearer tok_alice");
    expect(url).toBe(PORTAL_URL);
  });
});

describe("startCheckout", () => {
  it("opens the backend-produced checkout URL in the system browser", async () => {
    const fakeFetch = (async () =>
      new Response(JSON.stringify({ url: CHECKOUT_URL }), { status: 200 })) as unknown as typeof fetch;
    const open = vi.fn<UrlOpener>(async () => {});

    const url = await startCheckout("http://localhost:8787", "tok_alice", "pro", open, fakeFetch);

    expect(open).toHaveBeenCalledWith(CHECKOUT_URL);
    expect(url).toBe(CHECKOUT_URL);
  });

  it("does not open anything when the backend rejects the request", async () => {
    const fakeFetch = (async () =>
      new Response("", { status: 401 })) as unknown as typeof fetch;
    const open = vi.fn<UrlOpener>(async () => {});

    await expect(
      startCheckout("http://localhost:8787", "bad", "pro", open, fakeFetch),
    ).rejects.toThrow(BillingRequestError);
    expect(open).not.toHaveBeenCalled();
  });
});

describe("openPortal", () => {
  it("opens the backend-produced portal URL in the system browser", async () => {
    const fakeFetch = (async () =>
      new Response(JSON.stringify({ url: PORTAL_URL }), { status: 200 })) as unknown as typeof fetch;
    const open = vi.fn<UrlOpener>(async () => {});

    const url = await openPortal("http://localhost:8787", "tok_alice", open, fakeFetch);

    expect(open).toHaveBeenCalledWith(PORTAL_URL);
    expect(url).toBe(PORTAL_URL);
  });
});
