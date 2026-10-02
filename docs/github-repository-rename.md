# GitHub repository rename: `bernacle/jobhunt` → `bernacle/narrow`

A record of the 2026-10-02 rename and of what it touched. Only the
repository's identity changed. The crates, environment variables, file
locations, database and API keep their internal `jobhunt` name (see
"Intentionally unchanged" below).

1. **Old URL:** https://github.com/bernacle/jobhunt
2. **New URL:** https://github.com/bernacle/narrow

## 3. GitHub rename

`gh repo rename narrow -R bernacle/jobhunt`: a rename, not a new
repository. Before renaming, `bernacle/narrow` returned 404 (it was free).

| Check | Result |
| --- | --- |
| Repository ID | unchanged, `1386564103` |
| `github.com/bernacle/jobhunt` | 301 → `github.com/bernacle/narrow` |
| `api.github.com/repos/bernacle/jobhunt` | 301 → `api.github.com/repositories/1386564103` |
| Visibility, default branch | public, `main` (unchanged) |
| Issues / PRs / releases / tags | 0 / 39 / 0 / 0 (unchanged) |
| Workflows | CI, Live sources, Dependabot Updates: active |
| Ruleset "Protect main" (24132583) | identical before and after (rules, required checks, bypass) |
| Merge settings, security and analysis | identical before and after |
| Environments | `Production`, `Preview` (Vercel), `jobhunt / production` (Railway) |

Nothing that GitHub's rename handles specially was present: no published
Action (`action.yml`), no Pages, no repository webhooks, no deploy keys,
no forks, no releases, no Actions secrets or variables. Crates have
`publish = false`, and no workflow publishes packages or images.

**Never create a new `bernacle/jobhunt`.** It would take over the old
name and break GitHub's redirects.

## 4. Local remote

All the worktrees share one Git directory (`~/Dev/jobhunt/.git`), so one
change covers them all. The `github-personal` SSH host alias was kept:

```
git@github-personal:bernacle/jobhunt.git → git@github-personal:bernacle/narrow.git
```

`git fetch` and `git push` work against the new URL.

## 5. References changed in the repository

| File | Change | Why |
| --- | --- | --- |
| `.github/workflows/live.yml` | `github.repository == 'bernacle/jobhunt'` → `'bernacle/narrow'` (twice) | `github.repository` is now `bernacle/narrow`: scheduled live-source runs would otherwise have stopped silently |
| `.railway/railway.ts` | `github("bernacle/jobhunt", …)` → `github("bernacle/narrow", …)` | the IaC's service source (it is not applied automatically; production already follows the rename, see below) |
| `README.md` | feedback-issue link | public link |
| `docs/oss-adoption-experiment.md` | outreach messages and the clean-room note | public links in copy-paste messages |

## 6. Vercel

Project `narrow` (`prj_w00FZmXkq33KjtHxy0BbxXhsuWL6`): Root Directory
`apps/web`, production branch `main`, five production environment
variables, and the domains `narrow.fyi` (production), `www.narrow.fyi`
(redirects to it) and `narrow-delta.vercel.app`.

Vercel links the project by GitHub repository ID (`repoId: 1386564103`,
unchanged). Right after the rename the API still showed the cached name
(`repo: "jobhunt"`). Deployments are what matter, and they kept working:
see Verification. No reconnect, and no project changes.

The project is already named `narrow`, so there is nothing cosmetic to
rename. `narrow-delta.vercel.app` is the repository's GitHub "Website"
field. That works, but `https://narrow.fyi` would be the canonical
choice. Changing it is optional and unrelated to the rename.

## 7. Railway

Project `jobhunt` (`bacd0617-5d21-4a3f-902d-820977273e69`), environment
`production`:

| Service | Source before → after | Branch | Other config |
| --- | --- | --- | --- |
| `api` | `bernacle/jobhunt` → `bernacle/narrow` (automatic) | `main` | unchanged (start, pre-deploy, `/ready` healthcheck, domain, region, replicas) |
| `worker-discovery` | `bernacle/jobhunt` → `bernacle/narrow` (automatic) | `main` | unchanged (cron `7,37 * * * *`) |
| `worker-verification` | `bernacle/jobhunt` → `bernacle/narrow` (automatic) | `main` | unchanged (cron `22 * * * *`) |
| `Postgres` | image `postgres-ssl:18`, not from Git | n/a | `postgres-volume` untouched |

Railway updated the source slug on all three services by itself, so no
`connect-service-source` was needed. No variable value contains the
repository URL. The project has no webhooks.

**Names:** the project is still called `jobhunt`, which is purely
cosmetic. It shows up only in the dashboard, in GitHub's deployment
environment name (`jobhunt / production`) and in `project("jobhunt")` in
`.railway/railway.ts`. Private networking uses service names
(`api.railway.internal`), and no service name contains `jobhunt`.
Renaming the project would replace the GitHub environment
`jobhunt / production` with a new one, which has no operational value.
Optional, low priority.

## 8. Other services

| Service | Finding | Classification |
| --- | --- | --- |
| Linear (GitHub integration, `BRU-…` branches) | GitHub App, keyed by repository ID. Old PR attachment URLs redirect | automatic rename/redirect |
| Dependabot | `.github/dependabot.yml` has no slug | automatic |
| Secret scanning, push protection | unchanged | no action needed |
| OIDC provider, Resend (email) | configured by domain and keys, not by repository | no action needed |
| Codecov, Sentry, PostHog, Supabase, Cloudflare, Docker Hub, GHCR, Renovate, badges, uptime monitors, docs hosting, external CI | not used by this repository | not applicable |

## 9. Intentionally unchanged `jobhunt` identifiers

These are internal names. Renaming them would need a compatibility-aware
migration, and none of them is the repository's identity:

- crates and Rust paths (`jobhunt-cli`, `jobhunt_storage`, …) and `Cargo.lock`;
- the private root `package.json` name `jobhunt-infrastructure` (never published);
- `JOBHUNT_*` environment variables, in code, CI, Vercel and Railway;
- data and config locations (`…/jobhunt/jobhunt.db`, `…/jobhunt/config.toml`);
- the `jobhunt/<version>` User-Agent sent to job boards (tests assert it);
- CI's Postgres user/database `jobhunt`;
- the README's "JobHunt" prose, which the README declares on purpose;
- the Railway project name `jobhunt` (section 7);
- local folder names (`~/Dev/jobhunt`, `~/.t3/worktrees/jobhunt`), which
  Git does not care about.

## 10. Verification

PR #40 (the one that added this file) was the first push after the rename.

| Check | Result |
| --- | --- |
| Push to `bernacle/narrow` | works; `git fetch` and `git ls-remote` too |
| CI on the PR | every required check passed (`fmt`, `clippy`, `test`, `cloud`, `docs`, `msrv`, `web`, `e2e`, `ci-passed`); ruleset still enforced |
| Vercel preview on the PR | built from `bernacle/narrow`, READY; the bot comment appeared. The project link then read `repo: "narrow"` (same `repoId`) |
| Merge `6878594` to `main` | CI passed on `main` |
| Vercel production | deployment for `6878594` READY; same project, Root Directory, five env vars and three domains |
| Railway | `api`, `worker-discovery` and `worker-verification` each auto-deployed `6878594` from `bernacle/narrow` `main`: SUCCESS. Postgres and `postgres-volume` (5 GB) untouched |
| Health | `api` `/ready` 200; `narrow.fyi` 307 (sign-in redirect, as before); `www.narrow.fyi` 308 to the apex |
| Residual references | `git grep 'bernacle/jobhunt'` matches only this file and the "then named `bernacle/jobhunt`" note in `docs/oss-adoption-experiment.md` |

The `live.yml` guard is only exercised by the schedule (Mondays,
Wednesdays and Fridays, 06:17 UTC): the first scheduled run after the
rename should run, not be skipped.

## 11. Remaining manual actions

None are required. Optional: set the GitHub "Website" field to
`https://narrow.fyi` (Settings → General), and rename the Railway project
(section 7).

## 12. Rollback

- Rename back: `gh repo rename jobhunt -R bernacle/narrow`. This only
  works while nobody has created a new `bernacle/jobhunt`. GitHub then
  redirects `narrow` to `jobhunt`.
- `git remote set-url origin git@github-personal:bernacle/jobhunt.git`.
- Revert the PR that changed the files above (the `live.yml` condition
  must match the repository's name).
- Vercel and Railway follow renames by themselves. If one stops
  deploying, reconnect the existing project or service (Vercel: Project →
  Settings → Git; Railway: Service → Settings → Source). Never recreate it.
