/**
 * Typed client for the cloud authority's identity surface.
 *
 * Authority boundary (ADR-0001): the desktop never decides who the caller is —
 * it asks the cloud backend via `GET /me` and renders the answer. This module
 * is the read-only seam: it issues the authenticated request and parses the
 * resolved principal. It holds no secrets and makes no authorization decisions.
 */

/** The role a user holds within an account, as returned by the backend. */
export type Role = "owner" | "admin" | "member";

/** The resolved caller, mirroring the cloud API's `GET /me` response shape. */
export interface Me {
  user: { id: string; email: string };
  account: { id: string; name: string };
  membership: { role: Role };
}

/** The error thrown when the backend rejects or fails the `/me` request. */
export class MeRequestError extends Error {
  constructor(
    message: string,
    readonly status?: number,
  ) {
    super(message);
    this.name = "MeRequestError";
  }
}

const VALID_ROLES: ReadonlySet<string> = new Set(["owner", "admin", "member"]);

/**
 * Validate and narrow an unknown JSON value into a {@link Me}. The desktop
 * trusts the backend for *authority* but still validates the *shape* at the
 * boundary so a malformed response surfaces as a clear error, never as
 * `undefined` leaking into the UI.
 */
export function parseMe(value: unknown): Me {
  if (typeof value !== "object" || value === null) {
    throw new MeRequestError("malformed /me response: not an object");
  }
  const v = value as Record<string, unknown>;
  const user = asObject(v.user, "user");
  const account = asObject(v.account, "account");
  const membership = asObject(v.membership, "membership");

  const role = membership.role;
  if (typeof role !== "string" || !VALID_ROLES.has(role)) {
    throw new MeRequestError(`malformed /me response: unknown role ${String(role)}`);
  }

  return {
    user: { id: asString(user.id, "user.id"), email: asString(user.email, "user.email") },
    account: {
      id: asString(account.id, "account.id"),
      name: asString(account.name, "account.name"),
    },
    membership: { role: role as Role },
  };
}

function asObject(value: unknown, field: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null) {
    throw new MeRequestError(`malformed /me response: missing ${field}`);
  }
  return value as Record<string, unknown>;
}

function asString(value: unknown, field: string): string {
  if (typeof value !== "string") {
    throw new MeRequestError(`malformed /me response: ${field} is not a string`);
  }
  return value;
}

/**
 * Fetch the resolved principal from the cloud backend.
 *
 * Sends the bearer token in the `Authorization` header (the backend is the
 * authority; this is just transport). A non-2xx response — including 401 for an
 * unauthenticated request — is surfaced as a {@link MeRequestError} carrying the
 * status, so the caller can distinguish "not signed in" from "request failed".
 */
export async function fetchMe(
  baseUrl: string,
  token: string,
  fetchImpl: typeof fetch = fetch,
): Promise<Me> {
  const response = await fetchImpl(`${baseUrl}/me`, {
    headers: { Authorization: `Bearer ${token}` },
  });
  if (!response.ok) {
    throw new MeRequestError(`/me request failed`, response.status);
  }
  return parseMe(await response.json());
}
