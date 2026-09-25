import "server-only";

import { EncryptJWT, jwtDecrypt } from "jose";
import { cookies } from "next/headers";

import { config } from "./config";

/**
 * A signed-in session, kept in one encrypted, HttpOnly cookie (AES-256-GCM,
 * JWE `dir`). The access token never reaches browser JavaScript and nothing
 * lives in localStorage. There is no web-side session database: the account
 * is JobHunt Cloud's, resolved from the token by the API.
 */
export interface Session {
  accessToken: string;
  refreshToken?: string;
  /** Access token expiry, seconds since the epoch. */
  expiresAt: number;
  mode: "oidc" | "dev";
  /** For display only (from the identity provider's ID token). */
  name?: string;
  email?: string;
  /** For RP-initiated logout. */
  idToken?: string;
}

export const SESSION_COOKIE = "jh_session";
/** How long the cookie lives (refresh tokens keep the access token fresh). */
export const SESSION_MAX_AGE = 60 * 60 * 24 * 30;

async function key(): Promise<Uint8Array> {
  const digest = await crypto.subtle.digest(
    "SHA-256",
    new TextEncoder().encode(`jobhunt-web-session:${config().sessionSecret}`),
  );
  return new Uint8Array(digest);
}

/** Encrypts a small value for a cookie, valid for `maxAge` seconds. */
export async function seal(value: unknown, maxAge: number): Promise<string> {
  return new EncryptJWT({ s: value })
    .setProtectedHeader({ alg: "dir", enc: "A256GCM" })
    .setIssuedAt()
    .setExpirationTime(`${maxAge}s`)
    .encrypt(await key());
}

/** Decrypts what [`seal`] produced (`null` when invalid or expired). */
export async function unseal<T>(value: string | undefined): Promise<T | null> {
  if (!value) return null;
  try {
    const { payload } = await jwtDecrypt(value, await key());
    return (payload.s as T | undefined) ?? null;
  } catch {
    return null;
  }
}

export async function sealSession(session: Session): Promise<string> {
  return seal(session, SESSION_MAX_AGE);
}

export async function openSession(value: string | undefined): Promise<Session | null> {
  const s = await unseal<Session>(value);
  return s && typeof s.accessToken === "string" ? s : null;
}

export function cookieOptions(maxAge = SESSION_MAX_AGE) {
  return {
    httpOnly: true,
    secure: config().secureCookies,
    sameSite: "lax" as const,
    path: "/",
    maxAge,
  };
}

/** The current request's session, if any. */
export async function getSession(): Promise<Session | null> {
  const store = await cookies();
  return openSession(store.get(SESSION_COOKIE)?.value);
}

/** Sets the session cookie (route handlers and server actions only). */
export async function writeSession(session: Session): Promise<void> {
  const store = await cookies();
  store.set(SESSION_COOKIE, await sealSession(session), cookieOptions());
}

export async function clearSession(): Promise<void> {
  const store = await cookies();
  store.delete(SESSION_COOKIE);
}

/** Whether the access token expires within `margin` seconds. */
export function expiresSoon(session: Session, margin = 60): boolean {
  return session.expiresAt - margin <= Math.floor(Date.now() / 1000);
}
