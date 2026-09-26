# JobHunt web

The web app of JobHunt Cloud: **Today**, **Applications**, **Preferences**,
**Profile** and Settings. It is deliberately small. A good visit is short:
open Today, see the two to five opportunities that are new and worth your
time, understand why each one is there and what to watch out for, decide,
and leave. When nothing new is worth your time, it says you're caught up.

It decides nothing about jobs. Ranking, eligibility, verification, taste,
preference parsing and the evidence policy are the Rust application's
(`crates/jobhunt-app` and the domain crates), reached through the HTTP API
(`/api/v1`). This app renders their answers and forwards the person's
actions.

## Architecture

```text
browser ──(HTML, server actions; one encrypted HttpOnly cookie)──▶ Next.js server
                                                                     │  Bearer <access token>
                                                                     ▼
                                                        JobHunt API (/api/v1) ── Postgres
```

- **Next.js 16 App Router, React 19, TypeScript, Tailwind 4.** Pages are
  server components; the few interactive pieces (card actions, the reject
  dialog, forms) are small client components. No client-side data store:
  the API is the state.
- **Server-only API access** ([`src/lib/api.ts`](src/lib/api.ts)): pages and
  server actions call the API with the session's access token. The browser
  never sees a token or the API's private address.
- **Types from the server.** [`src/lib/api-schema.json`](src/lib/api-schema.json)
  is generated from the Rust response and request types
  (`crates/jobhunt-cloud/tests/api_schema.rs`, which fails when it is
  stale), and [`src/lib/api-types.ts`](src/lib/api-types.ts) from it
  (`npm run types`; `npm run types:check` in CI). A view changed in Rust
  that the web doesn't handle fails the type-check.
- **Auth** ([`src/lib/oidc.ts`](src/lib/oidc.ts),
  [`src/lib/session.ts`](src/lib/session.ts), [`src/proxy.ts`](src/proxy.ts),
  [`src/app/auth`](src/app/auth)): the API's identity provider (read from
  `/api/v1/auth/config`), authorization code + PKCE with `openid-client`,
  the API's audience requested, tokens sealed into one AES-256-GCM JWE
  cookie (HttpOnly, `SameSite=Lax`, `Secure` over https). The proxy renews
  the access token with the refresh token before it expires; a 401 clears
  the session. Development sign-in (`JOBHUNT_WEB_AUTH_MODE=dev`) mints a
  token from an API in development auth mode.
- **Mutations** are server actions ([`src/app/actions.ts`](src/app/actions.ts))
  that return the API's view or a stable error code;
  [`src/lib/errors.ts`](src/lib/errors.ts) turns codes into words. Raw
  server messages are not shown.
- **Freshness without real-time machinery:** Today renders on each visit,
  refreshes when you come back to the tab after five minutes, every 15
  minutes while visible, and on "Check again". Nothing polls in a hidden
  tab.

## Design direction

Calm, editorial, high-signal: typography and spacing over decoration, the
opportunity first, then why it may matter, then what to consider, then the
actions. Tiers are words ("Strong fit", "Worth reviewing"), never
percentages. Uncertainty is always shown (unverified listings, conditional
eligibility, unpublished pay). No gradients, no AI iconography, no metric
walls.

Every visual decision is a token in [`src/app/globals.css`](src/app/globals.css)
(colors for light and dark, the serif and sans stacks), mapped into
Tailwind's theme; components use only the semantic names (`bg-surface`,
`text-muted`, `border-line`, `text-caution`, …). BRU-305 can replace the
brand (type, color, logo) there without touching components. System font
stacks keep the app image- and font-download-free.

Accessibility is part of the components: landmarks and a skip link, one
`h1` per page, labelled forms, `aria-live` status for every action's
outcome, a native modal `<dialog>` for "Not for me" (focus moves in,
Escape closes), visible focus rings, reduced motion respected, and AA
contrast in both themes. Automated checks (axe) run in the component tests
and in the browser tests; they don't replace a manual review.

## Configuration

| Variable | |
| --- | --- |
| `JOBHUNT_API_URL` | where the server reaches the API (required) |
| `JOBHUNT_API_PUBLIC_URL` | the API's public URL, for the MCP connection instructions |
| `JOBHUNT_WEB_URL` | this app's URL (OAuth redirect: `<url>/auth/callback`) |
| `JOBHUNT_WEB_SESSION_SECRET` | 32+ characters (required) |
| `JOBHUNT_WEB_AUTH_MODE` | `oidc` (default) or `dev` |
| `JOBHUNT_WEB_OIDC_CLIENT_ID` / `_SECRET` | the web app's OAuth client (secret only for a confidential client) |
| `JOBHUNT_WEB_OIDC_SCOPES` | default `openid profile email offline_access` |

## Running it locally

Needs Node 24, a Postgres server (and `psql`), and the `jobhunt` binary
(`cargo build -p jobhunt-cli` at the repository root).

```bash
npm install
JOBHUNT_E2E_DATABASE_URL=postgres://user:pass@127.0.0.1:5432/postgres npm run stack
```

`npm run stack` ([`e2e/stack.mjs`](e2e/stack.mjs)) creates a fresh
`jobhunt_e2e` database, serves the fixture job boards
([`e2e/fixtures/boards.mjs`](e2e/fixtures/boards.mjs) and one recorded real
board) in Greenhouse's API format, runs `jobhunt migrate` and the real
discovery worker over them, starts `jobhunt server` (development auth,
email written to `.e2e/mail.jsonl`, verification against the fixtures),
and `next dev` on <http://127.0.0.1:3100>. Sign in with any name; import
`crates/jobhunt-resume/tests/fixtures/ana_lima.md` as the resume. Nothing
reaches a real job board or sends real email.

## Tests

```bash
npm run types:check   # the API types match the committed schema
npm run typecheck
npm run lint
npm test              # Vitest + Testing Library + axe (jsdom)
npm run build
npm run e2e           # Playwright against the real stack (needs `npm run build` and the binary)
```

- **Component tests** (`src/**/*.test.tsx`): the recommendation card (why,
  what to consider, verified vs listed vs unknown pay, eligibility
  uncertainty, verify-first notes, changed items), save / applied / not
  now / reject with a free-text reason and suggestions, rollback when the
  API refuses, the decision brief, the caught-up and "still gathering"
  states, the summary using only API counts, preference interpretation
  (understood, uncertain, not interpreted), learned vs stated taste,
  claim review, and axe checks on each.
- **End-to-end** (`e2e/*.spec.ts`, Chromium, desktop and a phone):
  sign in, upload a resume, state preferences, Today (a short list, only
  strong fits and jobs worth reviewing, verified pay and listing,
  stable across reloads, no infinite list), the full brief, reject with a
  reason from the keyboard, save, applied, Applications (stage changes),
  Preferences (stated vs learned), Profile (claim review), email
  notifications (confirmation link, nothing when nothing is new, one email
  for a strong job published later and found by the real discovery and
  verification workers, never twice), sign out and back in with everything
  kept, finishing Today to "caught up", signed-out redirects, a tampered
  cookie, no horizontal scrolling on a phone, and axe on every page.
