import "server-only";

import { redirect } from "next/navigation";

import type {
  AccountView,
  ApplicationContext,
  ClaimDecisionResult,
  ClaimReview,
  CreatedToken,
  ErrorBody,
  FeedView,
  FeedbackRequest,
  FeedbackResult,
  JobDetail,
  NotificationSettingsView,
  PipelineView,
  PreferenceUpdateResult,
  ProfileView,
  ResumeImportResult,
  TasteView,
  TokenList,
  UpdateNotificationsRequest,
  UpdatePreferencesParams,
} from "./api-types";
import { config } from "./config";
import { getSession } from "./session";

/**
 * A failed API call, with the API's stable error code (`no_profile`,
 * `unauthenticated`, `conflict`, …) or `cloud_unavailable` when JobHunt
 * Cloud could not be reached. Messages from the server are never shown
 * raw; `errors.ts` turns codes into words.
 */
export class ApiError extends Error {
  constructor(
    readonly code: string,
    readonly status: number,
    message: string,
    readonly hint?: string,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

type Body = { json: unknown } | { bytes: ArrayBuffer; contentType: string };

async function call<T>(method: string, path: string, body?: Body): Promise<T> {
  const session = await getSession();
  if (!session) throw new ApiError("unauthenticated", 401, "not signed in");
  const headers: Record<string, string> = {
    authorization: `Bearer ${session.accessToken}`,
    "x-jobhunt-client": "web",
  };
  let payload: BodyInit | undefined;
  if (body && "json" in body) {
    headers["content-type"] = "application/json";
    payload = JSON.stringify(body.json);
  } else if (body) {
    headers["content-type"] = body.contentType;
    payload = body.bytes;
  }
  let response: Response;
  try {
    response = await fetch(`${config().apiUrl}${path}`, {
      method,
      headers,
      body: payload,
      cache: "no-store",
      signal: AbortSignal.timeout(90_000),
    });
  } catch {
    throw new ApiError("cloud_unavailable", 503, "The Narrow API could not be reached");
  }
  if (response.status === 204) return undefined as T;
  const text = await response.text();
  if (!response.ok) {
    let error: ErrorBody["error"] | undefined;
    try {
      error = (JSON.parse(text) as ErrorBody).error;
    } catch {
      // Not our error body (a proxy's answer): classify by status.
    }
    const code = error?.code ?? (response.status >= 500 ? "cloud_unavailable" : "internal_error");
    throw new ApiError(code, response.status, error?.message ?? `HTTP ${response.status}`, error?.hint ?? undefined);
  }
  return JSON.parse(text) as T;
}

const q = encodeURIComponent;

export const api = {
  feed: (limit = 5) => call<FeedView>("GET", `/api/v1/feed?limit=${limit}`),
  opportunity: (id: string) =>
    call<JobDetail>("GET", `/api/v1/opportunities/${q(id)}?include_sources=true&full_description=true`),
  applicationContext: (id: string) =>
    call<ApplicationContext>("GET", `/api/v1/opportunities/${q(id)}/application-context`),
  feedback: (id: string, request: FeedbackRequest) =>
    call<FeedbackResult>("POST", `/api/v1/opportunities/${q(id)}/feedback`, { json: request }),
  dismiss: (id: string) => call<FeedbackResult>("POST", `/api/v1/opportunities/${q(id)}/dismiss`, { json: {} }),
  pipeline: (includeRejected = false) =>
    call<PipelineView>("GET", `/api/v1/pipeline${includeRejected ? "?include_rejected=true" : ""}`),
  profile: () => call<ProfileView>("GET", "/api/v1/profile"),
  claims: () => call<ClaimReview>("GET", "/api/v1/profile/claims"),
  decideClaims: (ids: string[], decision: "confirm" | "reject" | "reset", note?: string) =>
    call<ClaimDecisionResult>("POST", "/api/v1/profile/claims", { json: { ids, decision, note } }),
  uploadResume: (bytes: ArrayBuffer, fileName: string, contentType: string) =>
    call<ResumeImportResult>("PUT", `/api/v1/profile/resume?file_name=${q(fileName)}`, {
      bytes,
      contentType: contentType || "application/octet-stream",
    }),
  preferences: (update: UpdatePreferencesParams) =>
    call<PreferenceUpdateResult>("POST", "/api/v1/preferences", { json: update }),
  taste: () => call<TasteView>("GET", "/api/v1/taste"),
  account: () => call<AccountView>("GET", "/api/v1/account"),
  logoutEverywhere: () => call<void>("POST", "/api/v1/account/logout", { json: {} }),
  notifications: () => call<NotificationSettingsView>("GET", "/api/v1/notifications"),
  updateNotifications: (request: UpdateNotificationsRequest) =>
    call<NotificationSettingsView>("PUT", "/api/v1/notifications", { json: request }),
  confirmEmail: (token: string) =>
    call<NotificationSettingsView>("POST", "/api/v1/notifications/confirm", { json: { token } }),
  tokens: () => call<TokenList>("GET", "/api/v1/tokens"),
  createToken: (name: string, days: number) =>
    call<CreatedToken>("POST", "/api/v1/tokens", { json: { name, expires_in_days: days } }),
  revokeToken: (id: string) => call<void>("DELETE", `/api/v1/tokens/${q(id)}`),
  exportState: () => call<unknown>("GET", "/api/v1/export"),
};

/**
 * Runs a page's API calls; an expired or missing session goes back to
 * sign-in (clearing the cookie on the way) instead of an error page.
 */
export async function load<T>(work: () => Promise<T>): Promise<T> {
  try {
    return await work();
  } catch (error) {
    if (error instanceof ApiError && error.code === "unauthenticated") {
      redirect("/auth/expired");
    }
    throw error;
  }
}

/** Like [`load`], but a missing profile is a value, not an error. */
export async function loadOrNoProfile<T>(work: () => Promise<T>): Promise<T | "no_profile"> {
  try {
    return await load(work);
  } catch (error) {
    if (error instanceof ApiError && error.code === "no_profile") return "no_profile";
    throw error;
  }
}
