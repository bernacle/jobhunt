/** A same-site path to return to after sign-in (never another origin). */
export function safeNext(next: string | null | undefined, fallback = "/today"): string {
  if (!next || !next.startsWith("/") || next.startsWith("//") || next.startsWith("/\\")) return fallback;
  if (next.startsWith("/auth/") || next.startsWith("/signin")) return fallback;
  return next;
}
