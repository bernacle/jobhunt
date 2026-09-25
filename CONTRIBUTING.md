# Contributing

How changes get into `main` without breaking JobHunt. For what the product
does and how it is built, see the [README](README.md).

## Requirements

- Rust **1.88 or newer** (the workspace `rust-version`, the MSRV) via
  [rustup](https://rustup.rs), with the `rustfmt` and `clippy` components.
  Day-to-day development uses current stable.
- A C compiler, for the bundled SQLite and TLS libraries.

Nothing else: no Docker, no database server, no network access for tests.

## Build and test

```bash
cargo build --workspace                  # build everything
cargo test --workspace                   # the full offline suite (~280 tests, seconds once built)
cargo test -p jobhunt-storage            # one crate
cargo test -p jobhunt-cli --test multi_source_e2e   # one integration test file
```

## The quality gate: `./scripts/check.sh`

Run this before pushing, and before calling any task done (people and coding
agents alike). It runs the same checks as required CI, cheapest first,
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

[`.github/workflows/ci.yml`](.github/workflows/ci.yml) runs on every pull
request and every push to `main`. Its jobs are the required status checks:

| Check | What fails it |
| --- | --- |
| `fmt` | unformatted code |
| `clippy` | any clippy or compiler warning, on every target |
| `test` | a compile error (build step), then any failing unit, integration or doc test: model, lifecycle, dedupe, SQLite, migrations, adapters, end-to-end discovery, resume reading and parsing, profile re-import and evidence rules, the profile CLI flow, verification (domain, HTTP verifiers against mocks, SQLite), the eligibility rule matrix and region definitions, eligibility on real postings, and verify/show/check/find through the CLI |
| `docs` | any rustdoc warning (broken intra-doc links, …) |
| `msrv` | code or a dependency that needs a newer Rust than `rust-version` |
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
   `VERIFICATION_REVISION`. Schema changes are new, additive migrations; never
   edit an existing one.
3. Run `./scripts/check.sh`.
4. Open a PR against `main`. Merge when CI is green. A solo maintainer
   doesn't need approvals.

## Branch protection for `main`

Intended rules: protection against accidents, not bureaucracy.

- Changes arrive through pull requests; direct pushes, force pushes and
  branch deletion are blocked.
- Required status checks: `fmt`, `clippy`, `test`, `docs`, `msrv`,
  `ci-passed`, from GitHub Actions, and the branch must be up to date with
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
