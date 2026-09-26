/** Liveness for the platform's health check (no dependencies). */
export function GET() {
  return Response.json({ status: "ok" });
}
