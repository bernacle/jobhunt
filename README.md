# JobHunt

High-signal job discovery. JobHunt finds jobs from first-party sources,
normalizes them into one canonical model, stores them locally, and shows you
the ones that match.

## What it does today

`jobhunt find` fetches live postings from a set of company job boards hosted
on [Ashby](https://www.ashbyhq.com/), converts them into canonical jobs, saves
them to a local SQLite database, and prints the matches:

```text
$ jobhunt find engineer remote -n 2
Searching 8 sources…
 1. Product Engineer
    PostHog · Remote · Full-time
    https://jobs.ashbyhq.com/posthog/20ab9628-20ff-4ae3-bd6a-46ae7e9dc6b8
    ashby:posthog · posted today

 2. Senior Fullstack Software Engineer, Risk
    Vanta · Remote U.S. · Remote · Full-time
    $195K – $263K • Offers Equity • This role is also eligible for medical benefits, 401(k) plan, and other company perk programs.
    https://jobs.ashbyhq.com/vanta/bbaea4cf-1439-4996-b7b1-c10425413c3f
    ashby:vanta · posted yesterday

Showing 2 of 88 matching jobs. Use --limit to see more or add words to narrow the search.
Checked 8 sources in 1.1s: 581 open jobs (581 new, 0 updated).
```

The same command a second time ends with
`Checked 8 sources in 1.1s: 581 open jobs (0 new, 0 updated).`
Running it again does not duplicate anything: each job has a stable ID, so a
re-run updates `last_seen_at` for unchanged jobs and only rewrites jobs whose
content changed.

Not built yet: cross-source deduplication, verification against other
first-party sources, geographic eligibility, preference learning, ranking,
and the MCP server. The architecture leaves a place for each of these (see
below).

## Quick start

You need a stable Rust toolchain (1.88 or newer) and a C compiler (for the
bundled SQLite and TLS libraries). No database server or other setup is
required.

```bash
cargo build
cargo test
cargo run -- find
```

Useful variations:

```bash
cargo run -- find rust backend           # every word must match
cargo run -- find -n 50                  # show more results (default 20)
cargo run -- find --source ashby:openai  # search any Ashby board by name
cargo run -- find --offline designer     # search stored jobs, no network
cargo run -- config                      # show file locations and settings
cargo run -- --help
```

To install the binary: `cargo install --path crates/jobhunt-cli`.

### How search works

Each search word must match the start of a word in the job's title, company,
department, team, locations or workplace type, ignoring case and punctuation.
`rust` matches "Backend Engineer (Rust)" but not "Trust & Safety", and
`node.js` matches "Node.js".

After a fetch, `find` only shows jobs that were still listed during that fetch,
so jobs that have been taken down stop appearing. `--offline` shows everything
stored.

## Where data lives

| What | Default location |
| --- | --- |
| Database | macOS: `~/Library/Application Support/jobhunt/jobhunt.db`<br>Linux: `~/.local/share/jobhunt/jobhunt.db` |
| Config file (optional) | macOS: `~/Library/Application Support/jobhunt/config.toml`<br>Linux: `~/.config/jobhunt/config.toml` |

`jobhunt config` prints the exact paths on your machine. Use a different
database with `--database <PATH>` or `JOBHUNT_DATABASE`, and a different config
file with `--config <PATH>` or `JOBHUNT_CONFIG`. The database is created and
migrated automatically on first use; delete the file to start over.

## Configuration

Everything has a default, so no config file is needed. To customize, copy
[`config.example.toml`](config.example.toml) to the config path above. You can
change which Ashby boards are searched, where the database lives, logging,
timeouts and fetch concurrency. Settings are applied in this order, later
winning: built-in defaults, then the config file, then environment variables,
then command-line flags.

The Ashby API does not include company display names, so each configured
board can set `company`. Boards given only on the command line
(`--source ashby:<board>`) display the board name.

## Logging

Logs are structured (`tracing`) and go to stderr, so they never mix with
results on stdout. By default only errors are logged, because `find` already
reports failed sources and unreadable postings in plain language.

```bash
cargo run -- find -v                     # per-source progress and counts
cargo run -- find -vv                    # plus debug detail (HTTP, rejected records)
cargo run -- find -v --log-format json   # JSON lines
JOBHUNT_LOG="warn,jobhunt_sources=debug" cargo run -- find   # any tracing filter
```

At `-v`, each source logs fetch start/finish, how many postings it returned,
and how many were normalized, rejected, skipped, inserted, updated or
unchanged.

## Workspace layout

```text
crates/
  jobhunt-core      Domain-agnostic discovery primitives: canonical URLs, stable IDs,
                    content fingerprints, source keys/provenance, the Source trait,
                    ingest counters. Knows nothing about jobs.
  jobhunt-jobs      The jobs domain: canonical JobPosting/JobRecord, the JobRepository
                    trait (persistence boundary), and the Discovery pipeline.
  jobhunt-sources   Source adapters (Ashby today) and the shared HTTP client.
  jobhunt-storage   Storage backends. SQLite today, with migrations in
                    migrations/sqlite/.
  jobhunt-cli       The `jobhunt` binary: config, logging, commands, output.
  jobhunt-mcp       Placeholder for the MCP server; intentionally empty for now.
```

Dependencies only point downward: `cli → sources, storage → jobs → core`.
`jobhunt-jobs` does not depend on any HTTP or SQL crate.

### The discovery pipeline

```text
AshbySource::fetch          HTTP GET (timeouts, retries) → parse the board payload
  │                         → convert each posting into a canonical JobPosting
  │                           (URLs normalized, text cleaned, unknowns left as None)
  ▼                           a bad posting becomes a RecordError; the rest continue
Discovery::ingest           check invariants, drop in-batch duplicates
  ▼
JobRepository::upsert       one transaction per source: insert / update / unchanged
  ▼
jobhunt find                query the repository, print the results
```

Sources are fetched concurrently. If one source fails, the failure is reported
and the others continue. If storage fails, the run stops, so data is never
silently lost.

### Identity and deduplication

- **Canonical URLs** (`jobhunt_core::CanonicalUrl`): scheme and host are
  lowercased, and default ports, credentials, trailing slashes, anchor
  fragments and tracking parameters (`utm_*`, `gclid`, `gh_src`, `lever-source`,
  …) are removed. Remaining query parameters are sorted. `www.` and `http` are
  left alone because changing them can change the page.
- **Job IDs** look like `job_<32 hex>`. Each is a hash of the source (kind and
  board) plus the source's own ID for the posting, or the canonical URL if the
  source has no ID. The same posting always gets the same ID on every machine,
  so local and cloud databases will agree.
- **Fingerprints** hash a job's content. They are what distinguishes updated
  from unchanged on a re-run.

Deduplication across sources (the same job on Ashby and on the company site)
is intentionally not done yet.

### Local SQLite and cloud Postgres

The domain and pipeline talk to storage only through `JobRepository`, which
reports errors as `StorageError`, never as driver errors. The SQLite backend
stores timestamps as fixed-width UTC text and structured fields (locations,
compensation) as JSON, both of which map directly to Postgres
(`timestamptz`, `jsonb`). A Postgres backend would be a new module in
`jobhunt-storage` with its own `migrations/postgres/`, and the jobs domain
would not change.

The `jobs` table deliberately holds no per-user data. In a hosted setup a job
is fetched once for all users, and per-user state (saved jobs, ratings,
preferences) will live in separate tables that reference job IDs.

## Tests

```bash
cargo test                          # everything offline: unit, fixture, HTTP-mock, end-to-end
cargo test -p jobhunt-sources --test ashby_live -- --ignored --nocapture   # live Ashby API
JOBHUNT_LIVE_ASHBY_BOARD=openai cargo test -p jobhunt-sources --test ashby_live -- --ignored --nocapture
```

- `jobhunt-core`: URL normalization (including idempotency), stable IDs
  (checked against an independently computed value), fingerprints.
- `jobhunt-jobs`: ID derivation, validation, and pipeline behavior
  (idempotency, updates, rejections, failing sources) against an in-memory
  repository.
- `jobhunt-storage`: migrations, reopening a database, field round-trips,
  insert/update/unchanged outcomes, search.
- `jobhunt-sources`: parsing real saved Ashby responses
  (`tests/fixtures/ashby/`, including a file with deliberately broken
  records), plus HTTP behavior (404, retries, garbage bodies) against a local
  mock server.
- `jobhunt-cli`: config loading, output formatting, and an end-to-end test
  (adapter → pipeline → SQLite file) that runs discovery repeatedly.

Before sending changes, run:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test
```

## Adding a source

1. Add a module in `crates/jobhunt-sources/src/` implementing
   `jobhunt_core::Source<Record = JobPosting>`: fetch with the shared
   `HttpClient`, parse, convert. Follow `ashby.rs`: raw payload types with
   optional fields, per-record errors, and no invented values.
2. Add a variant to `SourceSpec` (in `from_key` and `build`) and a list to
   `SourcesConfig`.
3. Save real responses under `tests/fixtures/<source>/` and test the
   conversion against them. Add an `#[ignore]` live test.
