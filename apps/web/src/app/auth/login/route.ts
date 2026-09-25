import { NextResponse, type NextRequest } from "next/server";

import { config } from "@/lib/config";
import { startLogin } from "@/lib/oidc";
import { safeNext } from "@/lib/redirects";
import { cookieOptions, seal } from "@/lib/session";

const OAUTH_COOKIE = "jh_oauth";

/** Starts the identity provider's sign-in (authorization code + PKCE). */
export async function GET(request: NextRequest) {
  const next = safeNext(request.nextUrl.searchParams.get("next"));
  if (config().authMode === "dev") {
    return NextResponse.redirect(new URL(`/signin?next=${encodeURIComponent(next)}`, config().webUrl));
  }
  try {
    const login = await startLogin();
    const response = NextResponse.redirect(login.url);
    response.cookies.set(
      OAUTH_COOKIE,
      await seal({ verifier: login.verifier, state: login.state, next }, 600),
      cookieOptions(600),
    );
    return response;
  } catch (error) {
    console.error("sign-in could not start", error);
    return NextResponse.redirect(new URL("/signin?error=provider", config().webUrl));
  }
}
