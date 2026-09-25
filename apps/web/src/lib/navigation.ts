/**
 * The session ended mid-action: a full navigation to the route handler
 * that clears the cookie and sends the person to sign in (a client-side
 * transition can't clear an HttpOnly cookie).
 */
export function sessionExpired(): void {
  window.location.replace(new URL("/auth/expired", window.location.origin).href);
}
