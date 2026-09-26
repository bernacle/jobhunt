import "server-only";

/**
 * The web app's configuration, from the environment (Railway variables in
 * production). Everything here is server-side: the browser never sees the
 * API's address, a token or a secret.
 */
export interface WebConfig {
  /** Where the server reaches JobHunt Cloud's API (private network URL on Railway). */
  apiUrl: string;
  /** Where people and MCP clients reach the API (for the MCP connection instructions). */
  apiPublicUrl: string;
  /** This app's public URL (OIDC redirect URIs, links). */
  webUrl: string;
  /** `oidc` (production) or `dev` (sign in by name against a development API). */
  authMode: "oidc" | "dev";
  /** Encrypts the session cookie (32+ characters). */
  sessionSecret: string;
  /** The web app's OAuth client at the identity provider. */
  oidcClientId?: string;
  /** Its secret, when it is a confidential client (else PKCE only). */
  oidcClientSecret?: string;
  oidcScopes: string;
  secureCookies: boolean;
}

function required(name: string): string {
  const value = process.env[name]?.trim();
  if (!value) {
    throw new Error(`${name} is not set (see apps/web/README.md)`);
  }
  return value;
}

let cached: WebConfig | undefined;

export function config(): WebConfig {
  if (cached) return cached;
  const apiUrl = required("JOBHUNT_API_URL").replace(/\/+$/, "");
  const webUrl = (process.env.JOBHUNT_WEB_URL?.trim() || "http://localhost:3000").replace(/\/+$/, "");
  const authMode = process.env.JOBHUNT_WEB_AUTH_MODE?.trim() === "dev" ? "dev" : "oidc";
  const sessionSecret = required("JOBHUNT_WEB_SESSION_SECRET");
  if (sessionSecret.length < 32) {
    throw new Error("JOBHUNT_WEB_SESSION_SECRET must be at least 32 characters");
  }
  if (authMode === "oidc" && !process.env.JOBHUNT_WEB_OIDC_CLIENT_ID?.trim()) {
    throw new Error("JOBHUNT_WEB_OIDC_CLIENT_ID is not set (or use JOBHUNT_WEB_AUTH_MODE=dev locally)");
  }
  cached = {
    apiUrl,
    apiPublicUrl: (process.env.JOBHUNT_API_PUBLIC_URL?.trim() || apiUrl).replace(/\/+$/, ""),
    webUrl,
    authMode,
    sessionSecret,
    oidcClientId: process.env.JOBHUNT_WEB_OIDC_CLIENT_ID?.trim() || undefined,
    oidcClientSecret: process.env.JOBHUNT_WEB_OIDC_CLIENT_SECRET?.trim() || undefined,
    oidcScopes: process.env.JOBHUNT_WEB_OIDC_SCOPES?.trim() || "openid profile email offline_access",
    secureCookies: webUrl.startsWith("https://"),
  };
  return cached;
}
