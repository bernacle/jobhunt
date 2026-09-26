import { NextResponse, type NextRequest } from "next/server";

import { config } from "@/lib/config";
import { finishLogin } from "@/lib/oidc";
import { safeNext } from "@/lib/redirects";
import { SESSION_COOKIE, cookieOptions, sealSession, unseal } from "@/lib/session";

const OAUTH_COOKIE = "jh_oauth";

/** The identity provider sends the browser back here with a code. */
export async function GET(request: NextRequest) {
  const pending = await unseal<{ verifier: string; state: string; next: string }>(
    request.cookies.get(OAUTH_COOKIE)?.value,
  );
  const failed = (reason: string) => {
    const response = NextResponse.redirect(new URL(`/signin?error=${reason}`, config().webUrl));
    response.cookies.delete(OAUTH_COOKIE);
    return response;
  };
  if (!pending) return failed("expired");
  if (request.nextUrl.searchParams.get("error")) return failed("denied");
  try {
    // The URL the provider redirected to, as registered (behind Railway's
    // proxy the request's own host is internal).
    const callback = new URL(`${config().webUrl}/auth/callback${request.nextUrl.search}`);
    const tokens = await finishLogin(callback, pending.verifier, pending.state);
    const response = NextResponse.redirect(new URL(safeNext(pending.next), config().webUrl));
    response.cookies.delete(OAUTH_COOKIE);
    response.cookies.set(
      SESSION_COOKIE,
      await sealSession({
        accessToken: tokens.accessToken,
        refreshToken: tokens.refreshToken,
        expiresAt: tokens.expiresAt,
        name: tokens.name,
        email: tokens.email,
        mode: "oidc",
      }),
      cookieOptions(),
    );
    return response;
  } catch (error) {
    console.error("sign-in could not finish", error);
    return failed("provider");
  }
}
