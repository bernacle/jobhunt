// @vitest-environment node
import { beforeAll, describe, expect, it } from "vitest";

import { safeNext } from "./redirects";

beforeAll(() => {
  process.env.JOBHUNT_API_URL = "http://api.test";
  process.env.JOBHUNT_WEB_SESSION_SECRET = "unit-test-session-secret-0123456789abcdef";
  process.env.JOBHUNT_WEB_AUTH_MODE = "dev";
});

describe("the session cookie", () => {
  it("round-trips, and refuses anything tampered with", async () => {
    const { openSession, sealSession } = await import("./session");
    const session = { accessToken: "access-token-value", refreshToken: "refresh-token-value", expiresAt: 1_900_000_000, mode: "oidc" as const };
    const sealed = await sealSession(session);
    expect(sealed).not.toContain("access-token-value");
    expect(sealed).not.toContain("refresh-token-value");
    expect(await openSession(sealed)).toEqual(session);
    const tampered = `${sealed.slice(0, -4)}AAAA`;
    expect(await openSession(tampered)).toBeNull();
    expect(await openSession("not-a-session")).toBeNull();
    expect(await openSession(undefined)).toBeNull();
  });

  it("stays under the 4 KB cookie limit with provider-sized tokens", async () => {
    const { sealSession } = await import("./session");
    // An AuthKit-sized access token (RS256 JWT with several claims) and an
    // opaque refresh token.
    const accessToken = `eyJ${"x".repeat(1400)}.${"y".repeat(900)}.${"z".repeat(342)}`;
    const sealed = await sealSession({
      accessToken,
      refreshToken: "r".repeat(64),
      expiresAt: 1_900_000_000,
      mode: "oidc",
      name: "Ana Lima",
      email: "ana.lima@example.com",
    });
    expect(`jh_session=${sealed}`.length).toBeLessThan(4000);
  });
});

describe("returning after sign-in", () => {
  it("only goes back to this site", () => {
    expect(safeNext("/opportunities/opp_1")).toBe("/opportunities/opp_1");
    expect(safeNext("/settings/confirm?token=abc")).toBe("/settings/confirm?token=abc");
    for (const evil of ["https://evil.test", "//evil.test", "/\\evil.test", "/auth/login", "/signin?next=/x", "", null]) {
      expect(safeNext(evil)).toBe("/today");
    }
  });
});
