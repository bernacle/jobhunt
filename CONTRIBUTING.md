# Contributing

How changes get into `main` without breaking JobHunt. For what the product
does and how it is built, see the [README](README.md).

## Requirements

- Rust **1.88 or newer** (the workspace `rust-version`, the MSRV) via
  [rustup](https://rustup.rs), with the `rustfmt` and `clippy` components.
  Day-to-day development uses current stable.
- A C compiler, for the bundled SQLite and TLS libraries.

Nothing else for the local product: no Docker, no database server, no
network access for tests. The cloud tests also need a PostgreSQL server
(16 or newer) where the user may create databases: set
`JOBHUNT_TEST_DATABASE_URL` (e.g. `postgres://postgres:postgres@127.0.0.1/postgres`)
and they run; without it they are skipped. Each test creates and drops its
own database.

The web app ([`apps/web`](apps/web)) needs Node 24; its browser tests also
need a Postgres server, `psql`, and Chromium for Playwright
(`npx playwright install chromium`). See
[apps/web/README.md](apps/web/README.md).

## Build and test

```bash
cargo build --workspace                  # build everything
cargo test --workspace                   # the full offline suite (~400 tests, seconds once built)
cargo test -p jobhunt-storage            # one crate
cargo test -p jobhunt-cli --test multi_source_e2e   # one integration test file
```

## Where code goes

The CLI and the MCP server are interfaces to one local application:

- domain rules live in the domain crates (`jobhunt-jobs`, `-profile`,
  `-eligibility`, `-ranking`), storage in `jobhunt-storage`;
- use cases that both front-ends need (find, resolve an id, inspect,
  verify, record feedback, update preferences, application context,
  export/import) live in `jobhunt-app`, on `LocalApp`, together with the
  serializable views they return;
- `jobhunt-cli`, `jobhunt-mcp` and `jobhunt-cloud`'s HTTP handlers only
  parse arguments, call one `App` use case and present the answer. A rule
  written in a command, a tool handler or an API handler is a bug: the
  other interfaces would disagree.
- storage goes through the repository traits (`jobhunt_storage::Store`);
  a behavior both backends must share gets a case in
  `crates/jobhunt-storage/tests/contract.rs`, which runs it on SQLite and
  Postgres. Postgres schema changes are new migrations under
  `migrations/postgres`, like SQLite's.
- private data in Postgres is only reachable through `PgUserStore` (one
  account's view) and is sealed with `Keyring` before it is written; never
  log it (ids and counts only).
- the web app (`apps/web`) is another interface: it renders the API's
  views and calls its endpoints. It never ranks, filters, gates or
  re-words decisions (only presentation: dates, labels). When the web
  needs something new, add a use case and view in `jobhunt-app`, expose it
  in the API (and, when it makes sense, as an MCP tool), then regenerate
  the schema and types: `JOBHUNT_UPDATE_SCHEMA=1 cargo test -p
  jobhunt-cloud --test api_schema`, then `npm run types` in `apps/web`.
- email goes through `jobhunt_cloud::email::EmailSender`; tests use
  `MemorySender` (or the file provider in the browser tests). No test may
  send real email.

`jobhunt-app` and everything below it must never print: `jobhunt mcp`'s
stdout belongs to the protocol. Report progress through `Progress`, logs
through `tracing` (stderr). `mcp_protocol` runs the server at `-vvv` and
fails on any stdout line that isn't a JSON-RPC message.

Transactions that read and then write must start with
`SqliteJobStore::begin_write` (`BEGIN IMMEDIATE`), so a concurrent writer
is waited for instead of failing with `SQLITE_BUSY_SNAPSHOT`.

To try the MCP server by hand, `jobhunt doctor` prints the command, and
any MCP client (or the test harness in
`crates/jobhunt-cli/tests/common/mod.rs`) can drive it. For offline manual
runs, `JOBHUNT_DISCOVERY_ENDPOINT` and `JOBHUNT_VERIFY_ENDPOINT` send every
discovery and verification request to one base URL (a local mock serving
the fixtures); they are test hooks, not user settings.

## The quality gate: `./scripts/check.sh`

Run this before pushing, and before calling any task done (people and coding
agents alike). With `JOBHUNT_TEST_DATABASE_URL` set it also runs the
Postgres tests; `./scripts/check.sh --cloud` requires them (as CI's `cloud`
job does). It runs the same checks as required CI, cheapest first,
stops at the first failure, names it, and exits non-zero:

| Step | Command |
| --- | --- |
| formatting | `cargo fmt --all --check` |
| clippy | `cargo clippy --workspace --all-targets --locked -- -D warnings` |
| build | `cargo build --workspace --all-targets --locked` |
| unit, integration and doc tests | `cargo test --workspace --locked --no-fail-fast` (`--lib --bins`, `--test '*'`, `--doc`) |
| rustdoc | `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked` |

`./scripts/check.sh --msrv` also type-checks the workspace on the MSRV
(install it once with `rustup toolchain install 1.88 --profile minimal`).
`--web` adds the web app's checks (CI's `web` job) and `--e2e` also its
browser tests (CI's `e2e` job; `JOBHUNT_E2E_DATABASE_URL` names the
Postgres server, default `postgres://jobhunt:jobhunt@127.0.0.1:5432/jobhunt`).

Tests run with HTTP(S) proxies pointed at an unreachable address (local mock
servers excepted), so a test that accidentally reaches the internet fails
immediately rather than depending on a third party.

Fix formatting with `cargo fmt --all`.

### Optional pre-push hook

Not installed automatically. To run the gate before every `git push`:

```bash
printf '#!/bin/sh\nexec ./scripts/check.sh\n' > .git/hooks/pre-push
chmod +x .git/hooks/pre-push
```

Skip it once with `git push --no-verify`. CI remains the source of truth.

## Offline and live tests

Everything `cargo test` runs is offline and deterministic: saved real
responses under `crates/jobhunt-sources/tests/fixtures/`, resume fixtures
under `crates/jobhunt-resume/tests/fixtures/` (their README says how each
was made), local mock HTTP servers, and temporary SQLite files.

The live tests (`crates/jobhunt-sources/tests/*_live.rs`) are `#[ignore]`d
and read a few real boards per family. They check that the adapters still
understand today's payloads, so run them when changing an adapter:

```bash
cargo test -p jobhunt-sources --test greenhouse_live -- --ignored --nocapture
JOBHUNT_LIVE_LEVER_SITES=spotify cargo test -p jobhunt-sources --test lever_live -- --ignored --nocapture
```

The families are `ashby`, `greenhouse`, `lever` and `yc`; the README lists
the override variables.

The live verification test (`crates/jobhunt-eligibility/tests/verification_live.rs`,
also `#[ignore]`d) reads one listing per board, verifies up to
`JOBHUNT_LIVE_VERIFY_JOBS` (default 3) of its jobs through their
single-job endpoints and application pages, asks for a job that doesn't
exist (it must be "not found", not an error), and prints the location,
restriction and compensation facts read from each. Override the boards with
`JOBHUNT_LIVE_VERIFY_ASHBY`, `…_GREENHOUSE`, `…_LEVER`, `…_YC`:

```bash
cargo test -p jobhunt-eligibility --test verification_live -- --ignored --nocapture --test-threads 1
```

Offline, the `jobhunt` binary can be pointed at a local server for every
verification request with `JOBHUNT_VERIFY_ENDPOINT=http://127.0.0.1:<port>`
(the CLI end-to-end tests do). It is a test hook, not a user setting.

## Continuous integration

[`.github/workflows/ci.yml`](.github/workflows/ci.yml) runs on pull
requests that are ready for review (not on drafts) and on every push to
`main`. Its jobs are the required status checks:

| Check | What fails it |
| --- | --- |
| `fmt` | unformatted code |
| `clippy` | any clippy or compiler warning, on every target |
| `cloud` | any failing test against a real PostgreSQL 18 service: the storage contract on both backends, isolation, encryption, sync, leases, the HTTP API, authentication, hosted MCP, and the binary's cloud modes end to end (a missing database fails the job) |
| `test` | a compile error (build step), then any failing unit, integration or doc test: model, lifecycle, dedupe, SQLite, migrations, adapters, end-to-end discovery, resume reading and parsing, profile re-import and evidence rules, the profile CLI flow, verification (domain, HTTP verifiers against mocks, SQLite), the eligibility rule matrix and region definitions, eligibility on real postings, and verify/show/check/find through the CLI; ranking (job facets, reason reading, feedback state, learned taste, signals, gates, pay, briefs), feedback and rankings in SQLite, and rank/why/feedback/taste/pipeline through the CLI |
| `docs` | any rustdoc warning (broken intra-doc links, …) |
| `msrv` | code or a dependency that needs a newer Rust than `rust-version` |
| `web` | the web app: TypeScript types out of date with the API schema, a type error, a lint error, a failing component test (including axe checks), or a failed production build |
| `e2e` | the product loop failing in a real browser against the real stack (Postgres, `jobhunt server` and workers, fixture job boards, file email): sign in, onboarding, Today, feedback, Applications, Preferences, Profile, notifications, sign out/in, accessibility, a phone viewport |
| `ci-passed` | any of the above not succeeding |

Notes:

- `Cargo.lock` is committed and every CI command uses `--locked` (or
  `--frozen`), so CI builds exactly the dependency graph you tested. If CI
  says the lock file needs updating, run `cargo build` locally and commit
  `Cargo.lock`.
- The `msrv` job reads the version from `rust-version` in `Cargo.toml` and
  runs `cargo check --workspace --all-targets --locked` on it. That catches
  newer language or std features and dependencies whose own `rust-version`
  is higher. Tests are not repeated there: they already run on stable, and
  a compiler version change rarely changes runtime behavior, so running them
  twice would double CI time for little extra protection. Raising the MSRV is a
  deliberate change to `rust-version` (and the README).
- The other jobs use the latest stable Rust, so a new Rust release can bring
  new clippy lints. Fix them in a small, separate PR.
- A new push to a pull request cancels that PR's run in progress. Runs on
  `main` are never cancelled.
- Dependencies are cached with `Swatinem/rust-cache` (keyed on OS, rustc
  version, `Cargo.lock` and job). Only `main` writes the cache; pull
  requests reuse it.
- Dependabot opens one grouped PR a week for Cargo minor/patch updates and
  one a month for GitHub Actions. They go through the same checks,
  including `msrv`.

### Draft pull requests skip CI

The repository is private, so Actions minutes are limited (3,000 a month
on GitHub Pro), and one full run costs about 25 of them. CI runs when its
result matters: on pull requests that are ready for review and on `main`.

- **Draft pull requests skip every job**, `ci-passed` included. GitHub counts
  a skipped required check as passing. That's acceptable because a draft
  can't be merged, and marking it **Ready for review** runs the whole suite
  for real; those results replace the skips. Every push to a ready PR runs
  it again.
- The flow: open the pull request as a draft (`gh pr create --draft`), run
  `./scripts/check.sh` locally while iterating, and mark it ready
  (`gh pr ready`) when it's done. For a longer round of changes after
  that, convert it back to a draft (`gh pr ready --undo`).

### Live source validation

[`.github/workflows/live.yml`](.github/workflows/live.yml) runs the live
tests, one job per family (`live (ashby)`, `live (greenhouse)`, …) plus
`live (verification)`, on
Monday, Wednesday and Friday mornings (UTC) and on demand: **Actions → Live
sources → Run workflow**, optionally choosing one family (or
`verification`) and your own boards. It is never a required check and never runs on pull requests, so a
third-party outage cannot block a merge. A red run means a source changed,
went away, or was down. Look at the family's log, then re-run it or fix
the adapter (with a new fixture) in a PR.

## Pull request workflow

1. Branch from `main`.
2. Make the change with tests. A lifecycle, dedupe, storage or adapter
   change needs an offline regression test (fixture or mock server, not a
   live one); so does a change to resume parsing (a fixture under
   `crates/jobhunt-resume/tests/fixtures/`) or to the profile's re-import
   or evidence rules, and to an eligibility rule or the normalization it
   reads (a case in `crates/jobhunt-eligibility/tests/rule_matrix.rs`, and
   in `real_postings.rs` where a real posting shows it; region changes in
   `regions.rs`). A change that can change a decision bumps
   `RULES_VERSION` (`jobhunt-eligibility/src/decision.rs`) so stored
   decisions are not reused; a change to how verifications are read bumps
   `VERIFICATION_REVISION`. A ranking change needs a case in
   `jobhunt-ranking` (`rank/tests.rs`, or the facet, reason or taste
   tests next to the code) and bumps its revision so stored rankings are
   not reused: `RANKING_VERSION` (`rank.rs`) for signals, weights, gates
   and tiers, `TASTE_VERSION` (`taste.rs`) for how taste is learned, and
   `RULE_READER_REVISION` (`reason.rs`) for how reasons are read. Ranking
   never changes an eligibility decision. Schema changes are new, additive
   migrations; never edit an existing one.
3. Run `./scripts/check.sh`.
4. Open a **draft** PR against `main`; CI doesn't run on drafts. When the
   change is done, mark it ready for review, which runs CI. Merge when CI is
   green. A solo maintainer doesn't need approvals.

## Branch protection for `main`

Intended rules: protection against accidents, not bureaucracy.

- Changes arrive through pull requests; direct pushes, force pushes and
  branch deletion are blocked.
- Required status checks: `fmt`, `clippy`, `test`, `cloud`, `docs`, `msrv`,
  `web`, `e2e`, `ci-passed`, from GitHub Actions, and the branch must be up to date with
  `main` before merging.
- Zero required approvals.
- Repository admins can bypass the rules when merging a pull request (an
  emergency merge with red CI). They still can't push directly.

These are repository settings, not files, so they are not applied by
merging this repository. To apply them, go to **Settings → Rules →
Rulesets → New ruleset → Import a ruleset** and choose
[`.github/rulesets/main.json`](.github/rulesets/main.json). The checks only
show up after CI has run once. For admins to push directly in a real
emergency, change the bypass mode from "For pull requests only" to
"Always". Rulesets on a private repository need GitHub Pro or a paid
organization plan. On the free plan, make the repository public or rely on
the PR workflow above by convention.
