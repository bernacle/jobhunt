import "server-only";

import * as client from "openid-client";

import { config } from "./config";
import type { AuthConfigView } from "./api-types";

/**
 * Sign-in with the identity provider JobHunt Cloud trusts (WorkOS AuthKit
 * in production; any OpenID Connect / OAuth 2 provider works): the
 * authorization code flow with PKCE, from the server. The issuer and the
 * API's audience come from the API itself (`/api/v1/auth/config`), so the
 * web app cannot be configured to accept tokens the API would refuse, and
 * the account a person gets is the API's (the same `usr_…` as their CLI
 * and MCP clients).
 */

let authConfigCache: { value: AuthConfigView; at: number } | undefined;

export async function authConfig(): Promise<AuthConfigView> {
  if (authConfigCache && Date.now() - authConfigCache.at < 5 * 60_000) {
    return authConfigCache.value;
  }
  const response = await fetch(`${config().apiUrl}/api/v1/auth/config`, { cache: "no-store" });
  if (!response.ok) throw new Error(`JobHunt Cloud answered ${response.status} for its auth config`);
  const value = (await response.json()) as AuthConfigView;
  authConfigCache = { value, at: Date.now() };
  return value;
}

let providerCache: Promise<client.Configuration> | undefined;

async function discover(): Promise<client.Configuration> {
  const auth = await authConfig();
  if (auth.mode !== "oidc" || !auth.issuer) {
    throw new Error("JobHunt Cloud is not configured for OpenID Connect sign-in");
  }
  const { oidcClientId, oidcClientSecret } = config();
  const clientAuth = oidcClientSecret ? client.ClientSecretPost(oidcClientSecret) : client.None();
  const issuer = new URL(auth.issuer);
  try {
    return await client.discovery(issuer, oidcClientId!, undefined, clientAuth);
  } catch {
    // WorkOS AuthKit publishes RFC 8414 metadata rather than OpenID
    // discovery.
    return client.discovery(issuer, oidcClientId!, undefined, clientAuth, { algorithm: "oauth2" });
  }
}

export function provider(): Promise<client.Configuration> {
  providerCache ??= discover().catch((error) => {
    providerCache = undefined;
    throw error;
  });
  return providerCache;
}

function audienceParameters(auth: AuthConfigView): Record<string, string> {
  if (!auth.audience || !auth.audience_parameter) return {};
  return { [auth.audience_parameter]: auth.audience };
}

export interface LoginStart {
  url: string;
  verifier: string;
  state: string;
}

export async function startLogin(): Promise<LoginStart> {
  const [configuration, auth] = await Promise.all([provider(), authConfig()]);
  const verifier = client.randomPKCECodeVerifier();
  const state = client.randomState();
  const url = client.buildAuthorizationUrl(configuration, {
    redirect_uri: `${config().webUrl}/auth/callback`,
    scope: config().oidcScopes,
    code_challenge: await client.calculatePKCECodeChallenge(verifier),
    code_challenge_method: "S256",
    state,
    ...audienceParameters(auth),
  });
  return { url: url.href, verifier, state };
}

export interface Tokens {
  accessToken: string;
  refreshToken?: string;
  expiresAt: number;
  idToken?: string;
  name?: string;
  email?: string;
}

function tokensOf(response: client.TokenEndpointResponse & client.TokenEndpointResponseHelpers, previousRefresh?: string): Tokens {
  const claims = response.claims();
  const expiresIn = response.expiresIn() ?? 300;
  return {
    accessToken: response.access_token,
    refreshToken: response.refresh_token ?? previousRefresh,
    expiresAt: Math.floor(Date.now() / 1000) + expiresIn,
    idToken: response.id_token,
    name: typeof claims?.name === "string" ? claims.name : undefined,
    email: typeof claims?.email === "string" ? claims.email : undefined,
  };
}

export async function finishLogin(callbackUrl: URL, verifier: string, state: string): Promise<Tokens> {
  const [configuration, auth] = await Promise.all([provider(), authConfig()]);
  const response = await client.authorizationCodeGrant(
    configuration,
    callbackUrl,
    { pkceCodeVerifier: verifier, expectedState: state, idTokenExpected: false },
    audienceParameters(auth),
  );
  return tokensOf(response);
}

export async function refresh(refreshToken: string): Promise<Tokens> {
  const [configuration, auth] = await Promise.all([provider(), authConfig()]);
  const response = await client.refreshTokenGrant(configuration, refreshToken, audienceParameters(auth));
  return tokensOf(response, refreshToken);
}

/** Revokes the refresh token at the provider (best effort). */
export async function revoke(refreshToken: string): Promise<void> {
  try {
    const configuration = await provider();
    if (configuration.serverMetadata().revocation_endpoint) {
      await client.tokenRevocation(configuration, refreshToken);
    }
  } catch {
    // Signing out locally still happens.
  }
}

/** Where to send the browser to end the provider's session, if it has such an endpoint. */
export async function endSessionUrl(idToken?: string): Promise<string | null> {
  try {
    const configuration = await provider();
    if (!configuration.serverMetadata().end_session_endpoint) return null;
    return client.buildEndSessionUrl(configuration, {
      post_logout_redirect_uri: `${config().webUrl}/signin`,
      ...(idToken ? { id_token_hint: idToken } : {}),
    }).href;
  } catch {
    return null;
  }
}
