import { describe, expect, it } from "vitest";
import { fetchMe, MeRequestError, parseMe, type Me } from "./api";

const validMe: Me = {
  user: { id: "user_alice", email: "alice@example.com" },
  account: { id: "acct_acme", name: "Acme" },
  membership: { role: "owner" },
};

describe("parseMe", () => {
  it("narrows a well-formed /me payload into typed user/account state", () => {
    const parsed = parseMe({
      user: { id: "user_alice", email: "alice@example.com" },
      account: { id: "acct_acme", name: "Acme" },
      membership: { role: "owner" },
    });
    expect(parsed).toEqual(validMe);
  });

  it("rejects a payload that is not an object", () => {
    expect(() => parseMe(null)).toThrow(MeRequestError);
    expect(() => parseMe("nope")).toThrow(MeRequestError);
  });

  it("rejects a payload missing the account block", () => {
    expect(() =>
      parseMe({ user: { id: "u", email: "e" }, membership: { role: "owner" } }),
    ).toThrow(/account/);
  });

  it("rejects an unknown role", () => {
    expect(() =>
      parseMe({
        user: { id: "u", email: "e" },
        account: { id: "a", name: "n" },
        membership: { role: "superuser" },
      }),
    ).toThrow(/role/);
  });
});

describe("fetchMe", () => {
  it("sends the bearer token and returns the parsed principal", async () => {
    let seenUrl = "";
    let seenAuth: string | null = null;
    const fakeFetch = (async (url: string, init?: RequestInit) => {
      seenUrl = url;
      seenAuth = new Headers(init?.headers).get("Authorization");
      return new Response(JSON.stringify(validMe), { status: 200 });
    }) as unknown as typeof fetch;

    const me = await fetchMe("http://localhost:8787", "tok_alice", fakeFetch);

    expect(seenUrl).toBe("http://localhost:8787/me");
    expect(seenAuth).toBe("Bearer tok_alice");
    expect(me).toEqual(validMe);
  });

  it("throws a MeRequestError carrying the status on a 401", async () => {
    const fakeFetch = (async () =>
      new Response("", { status: 401 })) as unknown as typeof fetch;

    await expect(fetchMe("http://localhost:8787", "bad", fakeFetch)).rejects.toMatchObject({
      name: "MeRequestError",
      status: 401,
    });
  });
});
