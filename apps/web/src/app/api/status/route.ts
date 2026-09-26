import { NextResponse } from "next/server";

import { config } from "@/lib/config";

/**
 * Whether the API (and its database) answers, for the error page: in
 * production a failed render reaches the browser without its message, so
 * the page asks here to tell "can't reach Narrow" from other failures.
 */
export async function GET() {
  let reachable = false;
  try {
    const answer = await fetch(`${config().apiUrl}/ready`, { cache: "no-store", signal: AbortSignal.timeout(5_000) });
    reachable = answer.ok;
  } catch {
    reachable = false;
  }
  return NextResponse.json({ reachable }, { headers: { "cache-control": "no-store" } });
}
