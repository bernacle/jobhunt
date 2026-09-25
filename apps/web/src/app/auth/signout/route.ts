import { NextResponse, type NextRequest } from "next/server";

import { config } from "@/lib/config";
import { endSessionUrl, revoke } from "@/lib/oidc";
import { SESSION_COOKIE, openSession } from "@/lib/session";

/** Signs out of this browser: the refresh token is revoked at the provider. */
export async function POST(request: NextRequest) {
  const session = await openSession(request.cookies.get(SESSION_COOKIE)?.value);
  let target = `${config().webUrl}/signin?signed_out=1`;
  if (session?.mode === "oidc") {
    if (session.refreshToken) await revoke(session.refreshToken);
    target = (await endSessionUrl(session.idToken)) ?? target;
  }
  const response = NextResponse.redirect(target, 303);
  response.cookies.delete(SESSION_COOKIE);
  return response;
}
