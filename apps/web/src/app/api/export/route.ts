import { NextResponse } from "next/server";

import { ApiError, api } from "@/lib/api";

/** Downloads the portable state file (`GET /api/v1/export`). */
export async function GET() {
  try {
    const state = await api.exportState();
    const date = new Date().toISOString().slice(0, 10);
    return new NextResponse(JSON.stringify(state, null, 2), {
      headers: {
        "content-type": "application/json",
        "content-disposition": `attachment; filename="jobhunt-${date}.state.json"`,
        "cache-control": "no-store",
      },
    });
  } catch (error) {
    const status = error instanceof ApiError ? error.status : 500;
    return NextResponse.json({ error: error instanceof ApiError ? error.code : "internal_error" }, { status });
  }
}
