import { NextResponse, type NextRequest } from "next/server";

import { refresh } from "@/lib/oidc";
import { SESSION_COOKIE, cookieOptions, expiresSoon, openSession, sealSession } from "@/lib/session";

/**
 * Before every page: send people without a session to sign in, and renew
 * an access token about to expire with the refresh token (so pages and
 * server actions always call the API with a valid one). A session that
 * can't be renewed is cleared: the person signs in again.
 */
export async function proxy(request: NextRequest) {
  const { pathname, search } = request.nextUrl;
  const session = await openSession(request.cookies.get(SESSION_COOKIE)?.value);
  const toSignIn = (extra = "") => {
    const url = new URL("/signin", request.url);
    url.search = `?next=${encodeURIComponent(pathname + search)}${extra}`;
    const response = NextResponse.redirect(url);
    if (session) response.cookies.delete(SESSION_COOKIE);
    return response;
  };
  if (!session) return toSignIn();
  if (!expiresSoon(session)) return NextResponse.next();
  if (session.mode !== "oidc" || !session.refreshToken) return toSignIn("&expired=1");
  try {
    const tokens = await refresh(session.refreshToken);
    const sealed = await sealSession({
      ...session,
      accessToken: tokens.accessToken,
      refreshToken: tokens.refreshToken,
      expiresAt: tokens.expiresAt,
    });
    // This request's render sees the new token, and so does the browser.
    request.cookies.set(SESSION_COOKIE, sealed);
    const response = NextResponse.next({ request: { headers: request.headers } });
    response.cookies.set(SESSION_COOKIE, sealed, cookieOptions());
    return response;
  } catch {
    return toSignIn("&expired=1");
  }
}

export const config = {
  // Everything but sign-in, the auth routes, health and static files.
  matcher: ["/((?!signin|auth/|healthz|_next/static|_next/image|favicon.ico|icon.svg|robots.txt).*)"],
};
