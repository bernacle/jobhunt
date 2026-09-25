import { NextResponse } from "next/server";

import { config } from "@/lib/config";
import { SESSION_COOKIE } from "@/lib/session";

/** The API refused the session: forget it and ask to sign in again. */
export async function GET() {
  const response = NextResponse.redirect(new URL("/signin?expired=1", config().webUrl));
  response.cookies.delete(SESSION_COOKIE);
  return response;
}
