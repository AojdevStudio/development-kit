import { describe, expect, it } from "vitest";
import {
  allows,
  EntitlementsCache,
  EntitlementsRequestError,
  fetchEntitlements,
  parseEntitlements,
  type Entitlements,
} from "./entitlements";

const validResponse = {
  entitlements: {
    account_id: "acct_acme",
    plan: "pro",
    status: "active",
    trial: false,
    features: {
      export_pdf: true,
      cloud_sync: true,
      advanced_reports: true,
      max_projects: 100,
      team_members: 5,
    },
  },
};

const proEntitlements: Entitlements = {
  account_id: "acct_acme",
  plan: "pro",
  status: "active",
  trial: false,
  features: {
    export_pdf: true,
    cloud_sync: true,
    advanced_reports: true,
    max_projects: 100,
    team_members: 5,
  },
};

describe("parseEntitlements", () => {
  it("narrows a well-formed entitlements payload into typed access", () => {
    const parsed = parseEntitlements(validResponse);
    expect(parsed).toEqual(proEntitlements);
  });

  it("preserves an optional license_expires_at when present", () => {
    const parsed = parseEntitlements({
      entitlements: { ...validResponse.entitlements, license_expires_at: 1700604800 },
    });
    expect(parsed.license_expires_at).toBe(1700604800);
  });

  it("rejects a payload that is not an object", () => {
    expect(() => parseEntitlements(null)).toThrow(EntitlementsRequestError);
    expect(() => parseEntitlements("nope")).toThrow(EntitlementsRequestError);
  });

  it("rejects a payload missing the entitlements block", () => {
    expect(() => parseEntitlements({})).toThrow(/entitlements/);
  });

  it("rejects an unknown plan", () => {
    expect(() =>
      parseEntitlements({
        entitlements: { ...validResponse.entitlements, plan: "platinum" },
      }),
    ).toThrow(/plan/);
  });

  it("rejects an unknown status", () => {
    expect(() =>
      parseEntitlements({
        entitlements: { ...validResponse.entitlements, status: "bogus" },
      }),
    ).toThrow(/status/);
  });
});

describe("allows", () => {
  it("treats enabled booleans and non-zero limits as allowed", () => {
    expect(allows(proEntitlements, "export_pdf")).toBe(true);
    expect(allows(proEntitlements, "max_projects")).toBe(true);
  });

  it("denies unknown or off features", () => {
    expect(allows(proEntitlements, "priority_support")).toBe(false);
    const free: Entitlements = {
      ...proEntitlements,
      features: { export_pdf: false, team_members: 0 },
    };
    expect(allows(free, "export_pdf")).toBe(false);
    expect(allows(free, "team_members")).toBe(false);
  });
});

describe("fetchEntitlements", () => {
  it("sends the bearer token and returns the parsed entitlements", async () => {
    let seenUrl = "";
    let seenAuth: string | null = null;
    const fakeFetch = (async (url: string, init?: RequestInit) => {
      seenUrl = url;
      seenAuth = new Headers(init?.headers).get("Authorization");
      return new Response(JSON.stringify(validResponse), { status: 200 });
    }) as unknown as typeof fetch;

    const ent = await fetchEntitlements("http://localhost:8787", "tok_alice", fakeFetch);

    expect(seenUrl).toBe("http://localhost:8787/me/entitlements");
    expect(seenAuth).toBe("Bearer tok_alice");
    expect(ent).toEqual(proEntitlements);
  });

  it("throws an EntitlementsRequestError carrying the status on a 401", async () => {
    const fakeFetch = (async () =>
      new Response("", { status: 401 })) as unknown as typeof fetch;

    await expect(
      fetchEntitlements("http://localhost:8787", "bad", fakeFetch),
    ).rejects.toMatchObject({ name: "EntitlementsRequestError", status: 401 });
  });
});

describe("EntitlementsCache", () => {
  it("caches the last good value and serves it when a later fetch fails", async () => {
    const cache = new EntitlementsCache();
    const okFetch = (async () =>
      new Response(JSON.stringify(validResponse), { status: 200 })) as unknown as typeof fetch;

    // First load succeeds and populates the cache.
    const first = await cache.load("http://localhost:8787", "tok_alice", okFetch);
    expect(first).toEqual(proEntitlements);
    expect(cache.cached).toEqual(proEntitlements);

    // A subsequent failing fetch falls back to the cached value, not an error.
    const failFetch = (async () => {
      throw new Error("network down");
    }) as unknown as typeof fetch;
    const second = await cache.load("http://localhost:8787", "tok_alice", failFetch);
    expect(second).toEqual(proEntitlements);
  });

  it("re-throws when the very first fetch fails and there is no cached value", async () => {
    const cache = new EntitlementsCache();
    const failFetch = (async () =>
      new Response("", { status: 503 })) as unknown as typeof fetch;

    await expect(
      cache.load("http://localhost:8787", "tok_alice", failFetch),
    ).rejects.toBeInstanceOf(EntitlementsRequestError);
  });

  it("does NOT serve cached entitlements over a 401 — and clears the cache (ADR-0001)", async () => {
    // Authority boundary: a revoked token must never keep cached premium access.
    const cache = new EntitlementsCache();
    const okFetch = (async () =>
      new Response(JSON.stringify(validResponse), { status: 200 })) as unknown as typeof fetch;
    await cache.load("http://localhost:8787", "tok_alice", okFetch);
    expect(cache.cached).toEqual(proEntitlements);

    const deniedFetch = (async () =>
      new Response("", { status: 401 })) as unknown as typeof fetch;
    await expect(
      cache.load("http://localhost:8787", "tok_alice", deniedFetch),
    ).rejects.toMatchObject({ name: "EntitlementsRequestError", status: 401 });
    // The denial cleared the cache: no stale Pro lingering.
    expect(cache.cached).toBeUndefined();
  });

  it("does NOT serve cached entitlements over a 404 account-state denial", async () => {
    const cache = new EntitlementsCache();
    const okFetch = (async () =>
      new Response(JSON.stringify(validResponse), { status: 200 })) as unknown as typeof fetch;
    await cache.load("http://localhost:8787", "tok_alice", okFetch);

    const goneFetch = (async () =>
      new Response("", { status: 404 })) as unknown as typeof fetch;
    await expect(
      cache.load("http://localhost:8787", "tok_alice", goneFetch),
    ).rejects.toMatchObject({ status: 404 });
    expect(cache.cached).toBeUndefined();
  });

  it("does NOT serve cached entitlements over a malformed 200 response", async () => {
    // A 2xx whose body fails validation is not a trustworthy authoritative
    // answer — it must re-throw, not paper over with stale paid access.
    const cache = new EntitlementsCache();
    const okFetch = (async () =>
      new Response(JSON.stringify(validResponse), { status: 200 })) as unknown as typeof fetch;
    await cache.load("http://localhost:8787", "tok_alice", okFetch);

    const malformedFetch = (async () =>
      new Response(JSON.stringify({ entitlements: { plan: "platinum" } }), {
        status: 200,
      })) as unknown as typeof fetch;
    await expect(
      cache.load("http://localhost:8787", "tok_alice", malformedFetch),
    ).rejects.toBeInstanceOf(EntitlementsRequestError);
  });

  it("DOES serve cached entitlements over a transient 5xx", async () => {
    // A genuine transient outage is the one case the cache exists for.
    const cache = new EntitlementsCache();
    const okFetch = (async () =>
      new Response(JSON.stringify(validResponse), { status: 200 })) as unknown as typeof fetch;
    await cache.load("http://localhost:8787", "tok_alice", okFetch);

    const downFetch = (async () =>
      new Response("", { status: 503 })) as unknown as typeof fetch;
    const served = await cache.load("http://localhost:8787", "tok_alice", downFetch);
    expect(served).toEqual(proEntitlements);
  });
});
