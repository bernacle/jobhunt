# JobHunt

High-signal job discovery. JobHunt reads jobs from company job boards,
normalizes them into one canonical model, tracks how each one changes over
time, stores them locally, and shows you the ones that match.

## What it does today

`jobhunt find` reads every configured source (Ashby, Greenhouse and Lever
job boards, Y Combinator companies, and company careers pages that embed one
of those boards), converts the postings into canonical jobs, records what is
new, updated or gone, saves everything to a local SQLite database, and prints
the matches:

```text
$ jobhunt find engineer remote -n 2
Searching 17 sources…
 1. Senior Machine Learning Engineer, Trust
    Airbnb · San Francisco, CA · Remote
    https://careers.airbnb.com/positions/8232153?gh_jid=8232153
    greenhouse:airbnb · posted today · job_1e8575ded15556ca9b5ccf5485f05e4b

 2. Security Engineer - Detection and Response
    Spotify · New York, NY · Remote · Permanent
    https://jobs.lever.co/spotify/cb29d857-395b-401d-9749-367e666ff870
    lever:spotify · posted today · job_55f15065d06d5994d13e2433d877856f

Showing 2 of 209 matching jobs. Use --limit to see more or add words to narrow the search.
Checked 17 sources in 3.0s: 2867 open jobs (0 new, 0 updated).
```

Running it again is cheap and idempotent: boards that support it answer
"not modified" and nothing is re-read, nothing is duplicated, and only real
changes are reported (`… (3 new, 1 updated, 2 closed)`). A job listed by two
sources is shown once, with `also listed on <source>` under it.

`jobhunt show <job_…|opp_…>` prints everything stored about one job: every
source that lists it, when each first appeared and was last verified, and
its history (new, updated with the changed fields, closed, reopened).

Not built yet: resume/profile import, eligibility, preference learning,
ranking, and the MCP server.

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
cargo run -- find rust backend                       # every word must match
cargo run -- find -n 50                              # show more results (default 20)
cargo run -- find --source greenhouse:stripe         # one source, by KIND:NAME
cargo run -- find --source https://jobs.lever.co/spotify           # or by board URL
cargo run -- find --source https://www.notion.com/careers          # or by careers page
cargo run -- find --offline designer                 # search stored jobs, no network
cargo run -- find -v                                 # per-source statistics on stderr
cargo run -- show job_1e8575ded15556ca9b5ccf5485f05e4b             # one job, its sources and history
cargo run -- config                                  # show file locations and settings
```

To install the binary: `cargo install --path crates/jobhunt-cli`.

### How search works

Each search word must match the start of a word in the job's title, company,
department, team, locations or workplace type, ignoring case and punctuation.
`rust` matches "Backend Engineer (Rust)" but not "Trust & Safety", and
`node.js` matches "Node.js".

After a fetch, `find` shows open jobs that were listed during that fetch, one
per opportunity. Jobs of a source that failed are not shown for that run (a
warning names the source); they stay open in the database. `--offline` shows
every open stored job.

## Sources

| Kind | Instance | Reads | Complete listing? |
| --- | --- | --- | --- |
| `ashby` | board (`jobs.ashbyhq.com/<board>`) | `api.ashbyhq.com/posting-api/job-board/<board>` | yes, one response |
| `greenhouse` | board token (`job-boards.greenhouse.io/<board>`) | `boards-api.greenhouse.io/v1/boards/<board>/jobs?content=true&pay_transparency=true` | yes, when the job count matches `meta.total` |
| `lever` | site (`jobs.lever.co/<site>`) | `api.lever.co/v0/postings/<site>?mode=json` (or `api.eu.lever.co`) | yes, one response |
| `yc` | company slug (`ycombinator.com/companies/<slug>`) | `/companies/<slug>/jobs` plus each job's page | yes, per company |

All sources use plain HTTP through one shared client: a `jobhunt/<version>`
User-Agent, timeouts, bounded retries with exponential backoff for timeouts,
connection errors, 429 and 5xx (honoring `Retry-After`), at most 4 requests
in flight per host, and connection reuse. Nothing uses a browser or an LLM.

Source notes, from real data:

- **Ashby** does not return the company name; it comes from config.
- **Greenhouse** job content is HTML escaped once more; it is unescaped into
  `description_html` and converted to readable `description_text`.
  `absolute_url` is often the company's own page
  (`stripe.com/jobs/search?gh_jid=…`). Workplace and employment type exist
  only as company-specific custom fields ("Location Type", "Workplace Type",
  "Employment Type"), used when present. Pay ranges keep their labels
  ("Canada Annual Pay Range").
- **Lever** splits the description into opening, titled lists, "additional"
  and salary text; they are joined in page order. Commitments are free text:
  only unambiguous ones ("Full-time", "Contractor", "Internship") map, others
  ("Permanent", "Fixed-Term") are kept verbatim. `createdAt` is used as the
  posting date; Lever has no update time. Some sites return `[]` instead of
  404 when they have (or moved) no postings.
- **YC / Work at a Startup** has no jobs API. Its public pages are Inertia.js
  pages that embed their data as JSON in a `data-page` attribute; the adapter
  reads that JSON over HTTP (no HTML scraping, no browser). Descriptions are
  Markdown and only on each job's page, so a company with N jobs costs N+1
  requests. Posting dates are only published as relative text ("5 months"),
  so they stay unknown. The site-wide `/jobs` pages show a sample, not a
  complete list, and are not used.
- **Careers pages** are not scraped. JobHunt fetches the page, looks for a
  supported board it links to or embeds (`jobs.lever.co/<site>`,
  `boards.greenhouse.io/embed/job_board?for=<board>`,
  `jobs.ashbyhq.com/<board>`, …) and reads that board. Pages that only load
  their jobs with JavaScript (Stripe's, Anthropic's) can't be detected;
  configure their board instead.

Across the source families, unknown stays unknown: a field a source does not
publish is `None`, never guessed.

## Configuration

Everything has a default, so no config file is needed. To customize, copy
[`config.example.toml`](config.example.toml) to the config path shown by
`jobhunt config`. Settings are applied in this order, later winning: built-in
defaults, then the config file, then environment variables, then
command-line flags.

Sources are listed per family:

```toml
[[sources.ashby]]
board = "linear"
company = "Linear"        # display name; Ashby's API has none

[[sources.greenhouse]]
board = "stripe"          # company name comes from Greenhouse

[[sources.lever]]
site = "spotify"
company = "Spotify"
# region = "eu"           # for jobs.eu.lever.co sites

[[sources.yc]]
slug = "posthog"

[[sources.careers]]
url = "https://www.notion.com/careers"
```

With no `[sources]` at all, a built-in selection from every family is used.
Listing any source replaces all of the defaults, so the file fully controls
what is searched. Invalid names, unknown fields, duplicated sources and
non-http careers URLs are rejected with a message naming the problem.

`[discovery]` controls fetch concurrency (`concurrency`, default 8 sources;
`max_requests_per_host`, default 4), timeouts and retries, and
`revalidate_after_hours` (default 24, see below).

## Where data lives

| What | Default location |
| --- | --- |
| Database | macOS: `~/Library/Application Support/jobhunt/jobhunt.db`<br>Linux: `~/.local/share/jobhunt/jobhunt.db` |
| Config file (optional) | macOS: `~/Library/Application Support/jobhunt/config.toml`<br>Linux: `~/.config/jobhunt/config.toml` |

`jobhunt config` prints the exact paths on your machine. Use a different
database with `--database <PATH>` or `JOBHUNT_DATABASE`, and a different
config file with `--config <PATH>` or `JOBHUNT_CONFIG`. The database is
created and migrated automatically on first use; delete the file to start
over.

## Discovery lifecycle

```text
begin run
  │
Source::fetch(request)     adapter: HTTP fetch (conditional when possible), parse,
  │                        convert to canonical JobPostings; bad records become
  │                        per-record errors, the rest continue
  ▼
validate                   canonical invariants, drop in-batch duplicates,
  │                        decide whether this listing may close missing jobs
  ▼
JobRepository::apply_scan  lifecycle plan → one transaction per source:
  │                        rows, history, identity evidence, scan record
  ▼
identity::group            cross-source equivalence → opportunities
  ▼
finish run                 run statistics
```

Sources are fetched concurrently (bounded). A failing source is recorded and
reported; the others complete. Only a storage failure stops the run, so data
is never silently lost.

### NEW / UNCHANGED / UPDATED / CLOSED

Each source record (one job at one source) is classified on every scan of
its source ([`jobhunt_jobs::lifecycle`](crates/jobhunt-jobs/src/lifecycle.rs)):

| Stored state | In this scan | Result |
| --- | --- | --- |
| never seen for this source identity | listed | **NEW** |
| open, same material content | listed | **UNCHANGED** (only `last_seen_at` moves) |
| open, material content changed | listed | **UPDATED** |
| open | missing from a *trustworthy complete* listing | **CLOSED** |
| open | missing from a failed or partial scan | stays open |
| closed | listed again | **REOPENED** (open again; history shows both) |

"Material content" is title, company, URLs, department, team, locations,
employment and workplace type, remote flag, compensation, description text
and posting date. Markup-only changes to the description HTML and the
source's own update timestamp rewrite the stored row but are not UPDATED.

A listing can close jobs only when all of these hold:

1. the fetch succeeded and the adapter vouched the listing is complete
   (Greenhouse: job count equals `meta.total`; YC: the company page loaded);
2. every rejected record carried its source id (a record the source lists
   but that failed to convert is kept open, never closed; one without an id
   could be any job, so nothing closes);
3. it is not a sudden empty listing: an empty listing after a non-empty one
   closes nothing until a second consecutive empty listing confirms it.

When a source answers "not modified" (HTTP 304 to `If-None-Match`), its open
jobs are marked seen and nothing is re-read. That validator is only reused
from a listing that was fully applied, recorded under the current conversion
revision (`CANONICAL_REVISION`), and at most `revalidate_after_hours` old;
after that the listing is read in full again.

Closed jobs are never deleted. `find` hides them; `show` shows them.

## Identity and deduplication

There are two kinds of duplicates.

**The same job at the same source** is solved by stable ids. A job id
(`job_<32 hex>`) is a hash of the source (kind and instance) and the
source's own id for the posting (or the canonical URL when it has none). The
same posting always gets the same id on every machine, so repeated scans
update rows instead of adding them.

**The same job at different sources** is handled by grouping source records
into *opportunities* (`opp_<32 hex>`), without merging or changing the
records themselves
([`jobhunt_jobs::identity`](crates/jobhunt-jobs/src/identity.rs)). Two
records are linked only by deterministic evidence:

- the same ATS job recovered from their URLs: a Greenhouse job id
  (`boards.greenhouse.io/<board>/jobs/<id>`, `job-boards…`, embeds, or a
  first-party page's `?gh_jid=<id>`), a Lever posting id, an Ashby job id
  (`jobs.ashbyhq.com/<board>/<uuid>` or `?ashby_jid=`), or a Work at a
  Startup job id;
- an identical canonical posting URL or application URL.

False merges are worse than duplicates, so there are safeguards: a key two
records *of the same source* share (a generic "apply here" URL) is ignored;
linked records must have compatible titles (equal, or one a whole-word part
of the other); and two groups that each contain a record of one source are
never merged. Similar titles, companies or locations are never evidence on
their own. Records from different sources with the same company and title
but no shared evidence are counted as *look-alikes* in the statistics
(`find -v`) and left apart. PostHog's jobs on both Ashby and YC are the live
example: YC exposes no link to the Ashby posting, so they appear twice.

An opportunity is named after its earliest-seen record, so a job listed by
one source has `opp_` + its own job id's hex, and the id stays put as other
sources join. `find` shows one record per opportunity; `show` lists them all
with their own provenance.

**Canonical URLs** (`jobhunt_core::CanonicalUrl`): scheme and host are
lowercased, and default ports, credentials, trailing slashes, anchor
fragments and tracking parameters (`utm_*`, `gclid`, `gh_src`,
`lever-source`, `lever-via`, …) are removed; remaining parameters are sorted.
`www.`, `http`, path case and parameters that identify a job (`gh_jid`) are
kept. Host or path variants of one ATS posting are related by the identity
layer above, not folded into one URL.

## Database and history

SQLite, with migrations in
[`crates/jobhunt-storage/migrations/sqlite/`](crates/jobhunt-storage/migrations/sqlite/).
Migrations are additive; a database written by an earlier version is
upgraded in place (its jobs become open, get a `new` history entry, and are
baselined without being reported as UPDATED).

| Table | Holds |
| --- | --- |
| `jobs` | one row per source record: canonical content, status (`open`/`closed`), `first_seen_at`, `last_seen_at`, `content_updated_at`, `closed_at`, fingerprints, `opportunity_id` |
| `job_events` | history, one row per *change*: `new`, `updated` (with changed fields and the replaced version as a JSON snapshot), `closed`, `reopened` |
| `job_evidence` | identity keys (`url:…`, `ats:<system>:<id>`) used for grouping |
| `source_scans` | one row per source per run: listing / not modified / failed, complete or not, whether closing was applied or withheld and why, all counts, the validator |
| `discovery_runs` | one row per run with totals |

Unchanged observations add no history, so history grows with changes, not
with scans. The current row plus the chain of replaced snapshots answers:
when did this job first appear (`first_seen_at`), when was it last verified
(`last_seen_at`), did its pay or description change (`updated` events with
`compensation` / `description`), and was it closed and reopened.

The domain talks to storage only through `JobRepository`, which reports
`StorageError`, never driver errors; lifecycle decisions are made in the
domain, so a Postgres backend (its own module and `migrations/postgres/`)
would only persist them. Timestamps are fixed-width UTC text and structured
values JSON, both mapping directly to Postgres (`timestamptz`, `jsonb`). The
`jobs` table holds no per-user data: in a hosted setup a job is fetched once
for everyone, and per-user state belongs in separate tables keyed by job or
opportunity id.

## Logging

Logs are structured (`tracing`) and go to stderr, so they never mix with
results on stdout. By default only errors are logged.

```bash
cargo run -- find -v                     # per-source table and progress logs
cargo run -- find -vv                    # plus debug detail (HTTP, rejected records, links)
cargo run -- find -v --log-format json   # JSON lines
JOBHUNT_LOG="warn,jobhunt_jobs=debug" cargo run -- find   # any tracing filter
```

`-v` prints a table with, per source, the time taken and how many postings
were received, rejected, new, updated, unchanged, reopened and closed, and
whether the scan was a full listing, "not modified", partial, or had closing
withheld. The logs carry the same as events: `discovery run started`,
`source started`, `fetch completed`, `source not modified`,
`source completed`, `not closing missing jobs`, retries,
`duplicate detected`, `discovery run completed`.

## Workspace layout

```text
crates/
  jobhunt-core      Domain-agnostic primitives: canonical URLs, stable IDs,
                    fingerprints, HTML-to-text, source keys/provenance, the
                    Source trait (conditional fetch, completeness), counters.
  jobhunt-jobs      The jobs domain: canonical model, lifecycle rules,
                    cross-source identity, the JobRepository boundary, and the
                    Discovery pipeline.
  jobhunt-sources   Adapters (Ashby, Greenhouse, Lever, YC), careers-page board
                    detection, and the shared HTTP client.
  jobhunt-storage   Storage backends. SQLite today.
  jobhunt-cli       The `jobhunt` binary: config, logging, find, show, output.
  jobhunt-mcp       Placeholder for the MCP server; intentionally empty for now.
```

Dependencies only point downward: `cli → sources, storage → jobs → core`.
`jobhunt-jobs` does not depend on any HTTP or SQL crate.

## Tests

```bash
cargo test                          # everything offline
cargo test -p jobhunt-sources --test ashby_live -- --ignored --nocapture
cargo test -p jobhunt-sources --test greenhouse_live -- --ignored --nocapture
cargo test -p jobhunt-sources --test lever_live -- --ignored --nocapture
cargo test -p jobhunt-sources --test yc_live -- --ignored --nocapture
JOBHUNT_LIVE_GREENHOUSE_BOARDS=gitlab,databricks cargo test -p jobhunt-sources --test greenhouse_live -- --ignored --nocapture
```

The live tests read several real boards per family (override them with
`JOBHUNT_LIVE_ASHBY_BOARD`, `JOBHUNT_LIVE_GREENHOUSE_BOARDS`,
`JOBHUNT_LIVE_LEVER_SITES`, `JOBHUNT_LIVE_YC_COMPANIES`), print counts and
timings, and fail if a listing is incomplete, more than 5% of postings are
rejected, or any converted posting is invalid.

- `jobhunt-core`: URL normalization against real ATS URL variants (and URLs
  that must stay distinct), stable IDs, fingerprints, HTML-to-text.
- `jobhunt-jobs`: lifecycle rules, closing safeguards, conditional fetches,
  cross-source grouping and its safeguards, pipeline behavior against an
  in-memory repository.
- `jobhunt-storage`: migrations (including upgrading a first-release
  database), lifecycle persistence and history, scans, opportunities,
  search.
- `jobhunt-sources`: per adapter, conversion of saved real responses
  (`tests/fixtures/<family>/`, including files with deliberately broken
  records) and HTTP behavior against a local mock server (conditional
  requests, 404s, retries, truncated listings, garbage bodies, failed YC
  detail pages).
- `jobhunt-cli`: config, output, and end-to-end tests: every family served
  from one mock server through one pipeline into a SQLite file, then mutated
  across runs to prove NEW / UNCHANGED / UPDATED / CLOSED / REOPENED, that
  failed and partial scans close nothing, and cross-source grouping.

Before sending changes, run:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test
```

## Adding a source

1. Add a module in `crates/jobhunt-sources/src/` implementing
   `jobhunt_core::Source<Record = JobPosting>`: fetch with the shared
   `HttpClient` (use `get_conditional` if the source sends ETags), parse,
   convert. Follow `greenhouse.rs`/`lever.rs`: raw payload types with
   optional fields, per-record `RecordError`s, no invented values. Set
   `SourceBatch::complete` only when the batch is provably the whole
   listing, and `validator` to the response's ETag.
2. Add a `SourceSpec` variant (in `from_key`, `from_board`, `key`, `build`),
   a list on `SourcesConfig`, and the kind to `SUPPORTED_KINDS`. If its URLs
   carry a global job id, teach `identity::ats_job_ref` to read it, and
   `careers::board_for_url` to recognize its boards.
3. Save real responses under `tests/fixtures/<source>/`, test the conversion
   and HTTP behavior against them, and add an `#[ignore]` live test.
4. If the change alters what existing adapters produce, bump
   `CANONICAL_REVISION` so stored "not modified" validators are not reused.

Nothing in the pipeline, lifecycle, storage or CLI output changes.

## Known limitations

- YC companies must be listed individually. The directory of ~1,500 hiring
  companies is available (through the site's public search index), but
  scanning all of them costs thousands of page loads per run, so it is not
  done by default. YC pages carry per-request tokens, so they can't answer
  "not modified" and are re-read every run (listing + one page per job).
- Careers pages that render their jobs with JavaScript can't be resolved to
  a board; arbitrary first-party pages without an ATS behind them are not
  read.
- Cross-source grouping only uses URL and ATS-id evidence. Real duplicates
  without shared evidence (PostHog on Ashby and YC) are shown twice and
  counted as look-alikes.
- Grouping is recomputed over every stored record each run (fast for tens of
  thousands of records; a much larger corpus would need an incremental
  version).
- Location and remote data is kept as each source states it; it is not
  geocoded or normalized into countries, so eligibility filtering is future
  work.
- Greenhouse boards hosted in its EU region (`job-boards.eu.greenhouse.io`)
  are recognized in URLs but read through the global API, which does not
  serve them. Lever's EU region is supported (`region = "eu"`) but was not
  verified against a live EU site. Only public, unauthenticated endpoints
  are used.
