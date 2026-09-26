import { NextResponse, type NextRequest } from "next/server";

import { config } from "@/lib/config";
import { safeNext } from "@/lib/redirects";
import { SESSION_COOKIE, cookieOptions, sealSession } from "@/lib/session";

/**
 * Development sign-in: a name becomes an account on a JobHunt API running
 * in development auth mode (which refuses to start in production). For
 * local work and the end-to-end tests only.
 */
export async function POST(request: NextRequest) {
  const { authMode, apiUrl, webUrl } = config();
  if (authMode !== "dev") return new NextResponse("Not found", { status: 404 });
  const form = await request.formData();
  const name = String(form.get("name") ?? "").trim().slice(0, 64);
  const next = safeNext(String(form.get("next") ?? ""));
  if (!name) return NextResponse.redirect(new URL("/signin?error=name", webUrl), 303);
  const answer = await fetch(`${apiUrl}/api/v1/auth/dev-token`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ subject: name }),
    cache: "no-store",
  }).catch(() => null);
  if (!answer?.ok) return NextResponse.redirect(new URL("/signin?error=provider", webUrl), 303);
  const token = (await answer.json()) as { access_token: string; expires_in: number };
  const response = NextResponse.redirect(new URL(next, webUrl), 303);
  response.cookies.set(
    SESSION_COOKIE,
    await sealSession({
      accessToken: token.access_token,
      expiresAt: Math.floor(Date.now() / 1000) + token.expires_in,
      mode: "dev",
      name,
    }),
    cookieOptions(token.expires_in),
  );
  return response;
}
