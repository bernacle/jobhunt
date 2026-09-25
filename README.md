# JobHunt

High-signal job discovery. JobHunt reads jobs from company job boards,
normalizes them into one canonical model, tracks how each one changes over
time, stores them locally, and shows you the ones that match. It also keeps
a durable, inspectable model of *you*: your experience, the evidence behind
every professional claim, and what you want next.

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

`jobhunt verify <job_…|opp_…>` asks the job's authoritative sources
whether it is still open and can be applied to, records what they say,
and checks it against your profile: eligible, conditional, uncertain or
ineligible, with the posting's own words behind every reason (see
[Verification and eligibility](#verification-and-eligibility)). `jobhunt
check` prints the same report from what is stored; `show` includes the
last verification. With a profile, `find` adds a one-line verdict to every
result, and `--eligible` / `--possible` filter on it.

`jobhunt init resume.pdf` builds your career profile from your resume
(see [Career profile](#career-profile)): experiences, projects, education,
skills, domains, role signals, and an evidence graph where every claim keeps
the resume text it came from and your decision about it. `jobhunt
preferences add "…"` records what you want in your own words.

Not built yet: ranking and match scores, preference learning
from saved/rejected jobs, application assistance, resume tailoring, and the
MCP server.

## Quick start

You need a stable Rust toolchain (1.88 or newer, the declared minimum
supported version) and a C compiler (for the bundled SQLite and TLS
libraries). No database server or other setup is required.

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
cargo run -- verify job_1e8575ded15556ca9b5ccf5485f05e4b           # still open? can you take it?
cargo run -- verify --details job_1e8575ded15556ca9b5ccf5485f05e4b # with provenance and evidence
cargo run -- check job_1e8575ded15556ca9b5ccf5485f05e4b            # the same report, from what is stored
cargo run -- find --offline --eligible               # only jobs your profile says you can take
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
  so they stay unknown. Pay is published as text ("$190K - $215K"); a bare
  `$` does not say which dollar, so its currency stays unknown. The
  company's own visa field ("US citizen/visa only", "Will sponsor") is kept
  as `work_authorization`. The site-wide `/jobs` pages show a sample, not a
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
| Database (jobs and your profile) | macOS: `~/Library/Application Support/jobhunt/jobhunt.db`<br>Linux: `~/.local/share/jobhunt/jobhunt.db` |
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
`20260929000000_job_work_authorization.sql` adds the `work_authorization`
column (Work at a Startup's visa field); the canonical revision was bumped
so every source is read again and fills it.
`20260930000000_verification_eligibility.sql` adds the verification and
eligibility tables below without touching any existing table.

| Table | Holds |
| --- | --- |
| `jobs` | one row per source record: canonical content, status (`open`/`closed`), `first_seen_at`, `last_seen_at`, `content_updated_at`, `closed_at`, fingerprints, `opportunity_id` |
| `job_events` | history, one row per *change*: `new`, `updated` (with changed fields and the replaced version as a JSON snapshot), `closed`, `reopened` |
| `job_evidence` | identity keys (`url:…`, `ats:<system>:<id>`) used for grouping |
| `source_scans` | one row per source per run: listing / not modified / failed, complete or not, whether closing was applied or withheld and why, all counts, the validator |
| `discovery_runs` | one row per run with totals |
| `job_verifications` | one row per verification attempt: listing, application and authority states, whether it succeeded, the method, URL and failure kind as columns, and the whole record (authority chain, compensation facts, published places, unknowns) as JSON |
| `eligibility_decisions` | stored eligibility decisions, keyed by opportunity, profile and a digest of every input (see [Caching and revisions](#caching-and-revisions)) |

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

## Career profile

```text
$ jobhunt init resume.pdf
Imported resume.pdf (2 pages)
ignored 3 lines repeated on every page (header, footer, page numbers)
Marina Costa — Senior Software Engineer · Backend & Platform

Experience
  Ledgerly — Senior Software Engineer · Mar 2022 – Present · Remote
  Banco Horizonte — Software Engineer II · Jan 2020 – Apr 2022 · São Paulo, Brazil
  Banco Horizonte — Software Engineer · Jun 2018 – Dec 2019 · São Paulo, Brazil
  Full Stack Developer (freelance) · dates unknown

Skills
  PostgreSQL, AWS, Rust, Docker, Kafka, Kubernetes, React, Terraform, … (+9 more)

Preferences
  none yet

Evidence
  77 claims extracted
  56 directly supported
  21 need review

Uncertain
  - Full Stack Developer: no dates found

Next
  jobhunt profile
  jobhunt claims review
  jobhunt preferences add "I want … and at least …; avoid …"
```

Everything JobHunt believes about you can be inspected (`jobhunt profile`,
`jobhunt claims`), traced to its source (`jobhunt claims show <id>` prints
the resume sentence behind a claim), corrected (`jobhunt profile edit`,
`jobhunt claims reject`), and exported. Nothing needs an API key or the
network.

### Commands

| Command | Does |
| --- | --- |
| `jobhunt init <resume>` | Import a resume (PDF, `.txt` or `.md`), or re-import an updated one |
| `jobhunt profile [--all]` | Experience, projects, education, skills (used vs only listed), domains, role signals, preferences, evidence counts, what is missing or uncertain |
| `jobhunt profile edit basics\|experience\|project\|education …` | Correct a value (`--title`, `--start 2021-03`, `--end none`, `--current`, `--tech Rust,Go`, …) |
| `jobhunt profile add experience\|project\|education\|skill …` | Add what the resume does not say |
| `jobhunt profile remove <id>` | Delete what you added; reject (hide) what was imported |
| `jobhunt profile export [-o file]` / `import <file> [--replace]` | Versioned JSON, all or nothing |
| `jobhunt profile history` | Every import, decision and edit |
| `jobhunt claims [--kind K] [--state S] [--for <id>] [--all]` | List claims, marked ✓ usable, ? needs review, ✗ rejected |
| `jobhunt claims review [--all]` | Claims needing review, each with why JobHunt believes it and the resume text |
| `jobhunt claims show\|confirm\|reject\|reset <id>…` | Inspect or decide (`reject --reason …`) |
| `jobhunt claims add "…" [--for <id>] [--kind K]` / `edit <id> "…"` | State or reword a claim yourself |
| `jobhunt preferences` | Preferences by category, and your statements verbatim |
| `jobhunt preferences add "…"` | A preference statement in your own words |
| `jobhunt preferences set role\|compensation\|work-mode\|location\|region\|timezone\|relocation\|sponsorship\|company\|domain\|work-style …` | One structured preference |
| `jobhunt preferences remove <pref_…\|stmt_…>` | Remove a preference, or a statement and what was read from it |

Ids are printed short (`clm_3fa2b1c4`); any unique prefix works. `claim`
and `prefs` are accepted as aliases.

### Architecture

```text
jobhunt-cli ──► jobhunt-resume ──► jobhunt-profile ──► jobhunt-core
     │                                   ▲
     └──────► jobhunt-storage ───────────┘  (ProfileRepository for SQLite)
```

- **`jobhunt-profile`** is the domain: the model, the evidence policy,
  preferences and their parser, the re-import rules, the export format, and
  `ProfileService` (the use cases the CLI calls, and MCP and web will). It
  knows nothing about SQL or PDFs; storage is the `ProfileRepository` trait.
- **`jobhunt-resume`** reads files and parses them into
  `jobhunt_profile::ParsedResume`, the contract any resume parser fills in.
  It only structures the document and keeps its words; deciding what those
  words claim, and how far to trust them, is the domain's job.
- **`jobhunt-storage`** implements `ProfileRepository` on the same SQLite
  file as the jobs, in its own tables.

### Reading resumes

PDFs are read locally with `pdf-extract` (pure Rust): no browser, no OCR, no
LLM. JobHunt lays the text out itself from glyph positions: lines by
baseline in content-stream order, spaces from real gaps (so kerning does not
split words, whether the PDF writes space characters or, like TeX, places
every word separately), wide gaps as column breaks (right-aligned dates),
paragraph breaks from vertical gaps, ligatures normalized, and lines that
repeat at the top or bottom of pages (running headers, "Page 2 of 3")
removed. The PDF reader can panic on damaged files; it runs on its own
thread, so that becomes an error. Empty or image-only PDFs, damaged files,
password-protected files and unsupported formats (`.docx`, …) are refused
with a message saying what to do instead.

The deterministic parser (`jobhunt_resume::DeterministicParser`) finds
sections by their headings (English and Portuguese), the name, headline,
contacts and location in the header, and entries by their header lines
(dates, column gaps, separators such as `—`, `|`, " at "). A company line
followed by several titled roles is one company with several positions.
Titles and companies are told apart by title words; when neither side has
one the entry is flagged ambiguous rather than guessed. Bullets are list
items when the document marks them, otherwise sentences rebuilt from
wrapped lines. Missing dates stay missing, with a note; lines it does not
understand are reported, not dropped.

`ResumeParser` (resume structure) and `StatementParser` (preference
statements) are traits. An AI-assisted parser can be plugged in later
behind them; nothing in the profile depends on one, and `jobhunt init`
never needs an API key.

### The evidence graph

Every professional statement is a **claim** (`clm_…`) about the profile or
one experience, project or education entry:

| Kind | Example | Provenance |
| --- | --- | --- |
| employment, education, project | "Senior Software Engineer at Ledgerly (Mar 2022 – Present)" | extracted |
| accomplishment, responsibility | each resume bullet, verbatim | extracted |
| technology | "Used Kafka at Ledgerly" (a "Tech:" line or a bullet naming it) | extracted |
| skill | "Lists Go as a skill (Languages)" | extracted |
| domain | "Worked in payments at Ledgerly" | inferred |
| role | "Backend engineering experience at Ledgerly" | inferred |
| ownership | "Staff-level role at …", "Mentored or hired engineers at …" | inferred |
| other | certifications, awards, anything you add | extracted / user_entered |

Each claim records its **source** (the imported document and the resume's
own words, never a paraphrase), **provenance** (`extracted`, `inferred`,
`user_entered`), **confidence** (`high`/`medium`/`low`, with the **basis**
of an inference: "mentions “payment providers”, “PIX”"), and your
**verification** (`unverified`, `confirmed`, `rejected`).

The evidence policy (`Claim::standing`) decides what may later be used on
your behalf (application answers, tailoring):

1. rejected claims, and claims about records you rejected, are never used;
2. a claim whose source left the resume needs review, even if confirmed,
   until you confirm it again;
3. confirmed and user-entered claims are usable;
4. extracted claims are usable when quoted from the resume with high
   confidence ("directly supported");
5. everything else, including every inference, needs review.

Inference is never promoted to truth on its own. Skills are records with
their evidence: *used in your work* (a technology claim in an experience or
project), *added by you*, or *listed only*, plus when they were last used;
there are no proficiency scores. Domains and role signals (backend,
platform, full stack, …; senior, staff, technical leadership, mentorship,
…) are inferred claims, each with the evidence it rests on.

### Preferences

Preferences are structured values with a stance (`required`, `wanted`,
`acceptable`, `unwanted`): roles; compensation (minimum and target, amount,
ISO currency — never assumed from your country — period, employment or
contract); location (where you live, remote/hybrid/on-site, regions, time
zones, relocation, visa sponsorship); company and team kinds (startup,
early-stage, founder-led, product company, agency, consulting, small team,
…); domains you like or avoid; and work style (ownership, IC vs
management, greenfield vs maintenance, async, meetings, closeness to
product, on-call).

`jobhunt preferences add "I want small product teams and at least $120k.
Avoid pure SRE roles."` stores the statement verbatim, then reads it with
deterministic rules: clauses, their polarity ("avoid", "no", "open to",
"at least", …), and known values. Each preference read from it links back
to the statement and the clause it came from. Hedged or cue-less readings
are marked uncertain, and parts that could not be read are kept and shown.

Currencies are never assumed. A code or a symbol only one currency uses
(`USD 120k`, `$120k USD`, `US$`, `CA$`, `R$`, `€`, `£`) settles it. A bare
`$` (or `¥`) does not: USD, CAD, AUD, NZD, SGD, MXN and others all write
`$`. If the rest of the statement points to exactly one of them ("I live
in Toronto. At least $150k." → CAD), that reading is kept but marked
uncertain, with a note naming the words it rests on; otherwise the
currency stays unknown, the note says so, and `jobhunt profile` lists it
until you set it (`jobhunt preferences set compensation --minimum 120k
--currency USD`). Compensation will be a hard constraint later, so a
guessed currency could wrongly exclude or favor jobs.
A newer preference with the same key (say, a new minimum salary) replaces
the older one, which is kept as history. `jobhunt check` and `find` read
the location, work-mode, time-zone, relocation, sponsorship,
authorization and engagement preferences (see
[Verification and eligibility](#verification-and-eligibility)); ranking
will read the rest (pay, roles, companies) later.

### Re-importing a resume

Run `jobhunt init` again after changing your resume. It never deletes and
re-inserts:

- **Identity.** Experiences are matched by company and title (and, when the
  title changed, by company and start date, so a corrected title updates
  the record), projects by name, education by institution, skills by
  normalized name, and claims by their subject plus what they say. The same
  file imported twice changes nothing and duplicates nothing.
- **Source facts update**, except fields you edited by hand, which always
  win.
- **Decisions survive.** Confirmed claims stay confirmed; rejected claims
  stay rejected and never come back as trusted. If the statement of a
  one-per-record claim changes (a title or dates), your confirmation of the
  old statement does not carry over.
- **Nothing is deleted.** Records and claims the new resume no longer
  contains are marked stale; stale claims need review before they are used
  again. A reworded bullet is a new claim that points to the one it
  replaces. What you entered yourself (records, claims, preferences) is
  never touched by an import.

`init` prints what changed: new, updated, unchanged and stale counts, the
decisions and edits it kept, and anything you need to confirm again.

### Export format

`jobhunt profile export` writes one JSON document:

```json
{
  "format": "jobhunt.profile",
  "version": 1,
  "exported_at": "2026-09-25T12:00:00Z",
  "generator": "jobhunt 0.1.0",
  "profile": { "id": "prof_…", "name": "…", "revision": 7, … },
  "documents": [ { "id": "doc_…", "sha256": "…", "text": "…", … } ],
  "experiences": [ { "id": "exp_…", "company": "…", "meta": { "origin": "resume", "verification": "unverified", "edited_fields": [], … } } ],
  "projects": [ … ], "education": [ … ], "skills": [ … ],
  "claims": [ { "id": "clm_…", "kind": "accomplishment", "subject": { "type": "experience", "id": "exp_…" }, "provenance": "extracted", "verification": "confirmed", "source": { "document": "doc_…", "snippet": "…" }, … } ],
  "preferences": [ { "id": "pref_…", "value": { "type": "compensation", "bound": "minimum", "amount": 120000, "currency": "USD", "period": "year" }, "stance": "required", … } ],
  "statements": [ { "id": "stmt_…", "text": "I want small product teams …", "reading": "understood", … } ]
}
```

The file contains your resume's text and contact details. `jobhunt profile
import` checks the format name and version first, parses strictly
(unknown fields are errors), validates every reference (claim subjects,
sources, superseded claims, project experiences, preference statements),
and only then replaces the stored profile in one transaction; an invalid
file changes nothing. Replacing an existing profile needs `--replace`.

### Storage

The migration `20260927000000_career_profile.sql` adds, without touching
the jobs tables: `profiles`, `profile_documents` (with the extracted text),
`profile_experiences`, `profile_projects`, `profile_education`,
`profile_skills`, `profile_claims`, `profile_skill_evidence` (which claims
back which skill), `profile_preference_statements`, `profile_preferences`
and `profile_events` (history); `20260928000000_preference_notes.sql` adds
the note explaining how an ambiguous preference was read. Every table is keyed by `profile_id`; a
local install has one profile, but nothing prevents more. Records are
rows, with JSON only for small values read whole (contact lists, a
preference's typed value, lists of edited fields). Saving writes the whole
profile in one transaction (upsert by id, delete what is gone, rebuild the
derived skill evidence) and checks a revision number, so two concurrent
writers cannot silently overwrite each other. The rules live in the
domain, so a Postgres backend would only persist them.

## Verification and eligibility

Two separate questions, answered separately:

- **Verification**: is this job still open at a source that speaks for the
  employer, can it be applied to, and what does that source publish right
  now?
- **Eligibility**: can *you* work it, from where you are, on the terms the
  posting states?

A job can be verified active and ineligible ("US residents only"),
verified active and uncertain ("Remote", no scope), or eligible on paper
but not verified (and then not recommended). The two answers are never
collapsed into one state, and neither is a percentage.

```text
$ jobhunt verify job_02e51190085f8a9a0772e845ddd9f329
Verifying 1 source record at 1 source…
Senior / Staff Fullstack Engineer — Linear
job_02e51190085f8a9a0772e845ddd9f329 · opp_02e51190085f8a9a0772e845ddd9f329

Verification
  ✓ First-party Ashby listing active
  ✓ Application path active (published by the board's API)
  Verified 12 seconds ago

Location
  Remote: Europe
  Limited to: North America, Europe
  You: Berlin, Germany

Employment
  Employment: full time
  Contractor / EOR: not stated
  Visa sponsorship: not stated
  Time zone: flexible hours

Compensation
  Not published
  Verified from the first-party listing · first verification

Eligibility
  ELIGIBLE

Why
  ✓ Germany is within the listed Europe region
  ✓ Germany is within Europe, which the description allows
  • The posting says working hours are flexible

Conflicting information
  ! The location fields say remote in Europe, but the description says North America, Europe
```

For someone in Toronto the same job is `UNCLEAR`: the listing says Europe,
the description includes North America, and JobHunt can't tell which one
applies to this role, so it says so and shows both statements. For
someone in São Paulo it is `INELIGIBLE`: both exclude Brazil.

This is a compatibility signal computed from what the posting publishes
and what you told JobHunt. It is not legal advice and does not determine
work authorization.

### Commands

| Command | Does |
| --- | --- |
| `jobhunt verify <job_…\|opp_…>` | Verify every source record of the job now (reusing an attempt from the last 15 minutes), save the result, and check it against your profile |
| `jobhunt verify --force <id>` | Ask the sources even if a recent attempt exists |
| `jobhunt verify --details <id>` | Plus provenance: every record, the method and URLs checked, the authority chain, what changed, and the posting's words and your profile facts under every reason |
| `jobhunt check <id>` | The detailed report from what is stored, without fetching (`--refresh` verifies first) |
| `jobhunt show <id>` | Everything stored, with the last verification and the eligibility verdict (no fetch) |
| `jobhunt find --eligible` / `--possible` | Only eligible or conditional jobs / everything not ruled out |
| `jobhunt preferences set authorized-in <country>` | A country (or "the EU") you may already work in |
| `jobhunt preferences set engagement contractor\|employee --stance require\|want\|accept\|avoid` | How you can be hired |

### What "verified" means

A verification asks the job's authoritative source directly, through the
narrowest public endpoint each family has, using the same HTTP client
(timeouts, bounded retries honoring `Retry-After`, at most 4 requests per
host) and the same conversion code as discovery:

| Family | Listing | Application path |
| --- | --- | --- |
| Greenhouse | `boards-api.greenhouse.io/v1/boards/<board>/jobs/<id>`; 404 is closed | the board's hosted job page (it holds the form); a redirect to the board with `error=true` is closed |
| Lever | `api.lever.co/v0/postings/<site>/<id>` (or `api.eu.lever.co`); 404 is closed | the posting's `/apply` page |
| Ashby | the board (Ashby has no single-job endpoint; one request, the whole board); a job missing from it is closed | the `applyUrl` the board API publishes (the page renders in the browser, so it is not requested) |
| YC / Work at a Startup | the job's page (its embedded page data); 404, or the company's job list instead, is closed | the Work at a Startup application URL the page names |

Verifying one job never re-reads more than that job's own endpoint (or one
Ashby board). Records of one opportunity are verified concurrently
(bounded, `[verification] concurrency`).

Each attempt is stored as a **verification record**
(`jobhunt_jobs::verification::VerificationRecord`), never updated or
deleted:

| Field | Holds |
| --- | --- |
| `listing` | `active`, `closed` (the source said so), `unreachable` (timeout, 5xx, connection), `ambiguous` (an unreadable answer, or a different record than the job), `unknown` (no verifier for the source) |
| `application` | `active`, `closed`, `unavailable`, `unknown`, and *how* it is known: `probed` (requested, with the HTTP status), `published` (the source publishes the route), `listing_page`, `not_checked` |
| `authority`, `authority_chain` | see below; each link records whether JobHunt requested it or the source only named it |
| `checked_url`, `listing_url`, `method` | what was asked, and how |
| `changed_fields`, `changed_since_last_verification`, `lifecycle` | what differs from the stored record and from the previous successful verification, and the lifecycle outcome |
| `compensation` | see [Compensation](#compensation-verification) |
| `published` | the location, workplace, remote, employment and work-authorization text the source publishes now, verbatim |
| `unknowns`, `failure` | what could not be established; a typed failure (`timeout`, `unavailable`, `malformed`, `request`, `mismatch`, `not_supported`) with the URL and HTTP status |
| `revision` | the verification rules revision (`VERIFICATION_REVISION`) |

An attempt **succeeds** when the source gives a definitive answer (active
or closed). The latest attempt and the latest successful verification are
kept apart, so a failure never erases an earlier success: "Last attempt 2
minutes ago failed; last successfully verified 3 days ago".

**Lifecycle.** A live posting is fed back through the existing discovery
lifecycle exactly as a scan listing only that job would be: an edited job
is UPDATED (with the changed fields and the replaced version in its
history), a closed job that is live again is REOPENED with its history
kept, and `last_seen_at` moves. No scan is recorded and nothing else is
closed. A job verified closed is not closed in the jobs table (closing is
discovery's decision, with its safeguards); the trust view shows it as
closed. A job discovery closed after its last successful verification is
never shown as verified active.

**Freshness** (`[verification]` in the config, one policy in
`FreshnessPolicy`): an attempt within `reuse_minutes` (15) is reused
instead of asking again (`--force` always asks), a success within
`fresh_hours` (24) is fresh, and older than `stale_hours` (72) it is stale
and the job is not trusted enough to recommend until verified again.
`show`, `check` and `find` only read what is stored.

### Authority

Who stands behind a listing, strongest first:

| Level | Means | Sources |
| --- | --- | --- |
| `employer_first_party` | a page on the employer's own domain, checked by JobHunt, that publishes the job or embeds its board | (no verifier checks employer pages yet) |
| `employer_configured_ats` | the employer's own applicant-tracking board, which it configures and publishes | Ashby, Greenhouse, Lever |
| `trusted_source` | a platform the employer posts to itself, but not its own system | Y Combinator's Work at a Startup |
| `secondary_source` | a reposting by someone else | none today |
| `unknown` | a source JobHunt doesn't know | |

A Greenhouse board that publishes the employer's own page as the job's URL
(`stripe.com/jobs/search?gh_jid=…`) adds that page to the chain, marked as
named by the board, not checked; the authority stays the board's. Work at
a Startup is never called first-party.

**Opportunities.** Every source record of an opportunity is verified and
kept inspectable. The opportunity's status rests on the strongest live
record (authority, then the most recent success); records that disagree
(one live, one gone) are listed as conflicts. It is **trusted enough to
recommend** only when that record is verified active, not stale, at a
source of at least `trusted_source` authority.

### Location normalization

Places are normalized from the job's structured location fields (and the
source's structured country), sentences of its description, and your
`current_location` preference (or, failing that, your resume's header,
which is then named in every reason that uses it). A place is never
guessed from the machine's locale or IP.

A normalized place is an area of the table in
[`geo`](crates/jobhunt-eligibility/src/geo.rs): anywhere, a region, a
country (ISO 3166-1 alpha-2 codes internally), a first-level subdivision,
or a city, with standard-time UTC offsets (per city in countries that span
several zones). The raw text is kept next to it, and text the table
doesn't know stays unrecognized (`JobHunt doesn't recognize your location
“…”`), never approximated. ISO codes are used as identifiers, not as
political statements.

### Region definitions

Membership lives in one tested table (`Region::members`). "Maybe" means
usage disagrees; a decision built on it is uncertain, never eligible or
ineligible.

| Region | Includes | Maybe |
| --- | --- | --- |
| North America | US, Canada | Mexico |
| Central America | Belize, Costa Rica, El Salvador, Guatemala, Honduras, Nicaragua, Panama | Mexico |
| Caribbean | Cuba, Dominican Republic, Jamaica, Trinidad and Tobago, Puerto Rico | |
| South America | Argentina, Bolivia, Brazil, Chile, Colombia, Ecuador, Guyana, Paraguay, Peru, Suriname, Uruguay, Venezuela | |
| Latin America (LATAM) | Mexico, Central and South America, the Spanish-speaking Caribbean | Belize, Guyana, Suriname, Jamaica, Trinidad and Tobago |
| Americas | all of the above | |
| Europe | the EU, the EEA, the UK, Switzerland, the Western Balkans, Ukraine, Moldova | Turkey, Russia, Belarus |
| EU | the 27 member states | other European countries (postings often write "EU" for Europe) |
| EEA | the EU plus Iceland, Liechtenstein, Norway | (formal: nothing else) |
| Nordics / DACH | Sweden, Norway, Denmark, Finland, Iceland / Germany, Austria, Switzerland | — / Liechtenstein |
| Middle East | UAE, Saudi Arabia, Israel, Qatar, Kuwait, Bahrain, Oman, Jordan, Lebanon, Syria, Iran | Turkey, Egypt |
| Africa | the African countries in the table | |
| EMEA | Europe (including its maybes), the Middle East (including its maybes), Africa | |
| Asia | East, Southeast and South Asia | the Middle East, Turkey |
| APAC | East and Southeast Asia, Oceania, India | Pakistan, Bangladesh, Sri Lanka, Nepal |
| Oceania | Australia, New Zealand | |
| Global / anywhere | every country | |

### Remote scope

Remote availability is separate from workplace type. A job offers one or
more **work options**:

- **remote**, with a scope: explicitly global ("Remote - Worldwide",
  "Anywhere"), a list of areas ("Remote (US)", "Remote - LATAM", "Remote:
  Brazil, Argentina, Chile"), or **unknown** ("Remote" alone). A remote
  job that lists only an office city ("New York, NY") has a scope
  *inferred* from that city's country, which is never enough for a
  definite answer and gives way to an explicit statement in the
  description;
- **office** (on-site, hybrid, or office-based when the source doesn't
  say how often), at a place;
- **engagement**: a remote path through a named mechanism in named places
  ("we hire contractors in Brazil, Argentina and Mexico through Deel").

"Remote" is never "anywhere", "Remote — US" is not "Remote", and
"Americas" is not "Global". An explicit hybrid or on-site workplace type
outranks a remote flag (Ashby sets `isRemote` on some hybrid jobs; the
disagreement is recorded).

### Restrictions

The description is read with fixed English cues, sentence by sentence,
keeping each sentence as evidence:

- **allowed places** ("must be based in Canada", "US only", "open to
  candidates in Brazil", "we're hiring in LATAM", "Remote in the US",
  "applicants must live in NYC"), each *required* or only *preferred*
  ("candidates in Europe are preferred" is not a rule);
- **excluded places** ("we are unable to hire in Cuba, Iran, North Korea
  or Syria");
- **anywhere** ("work from anywhere", "anywhere in the world"), but not
  descriptions of the team ("a globally distributed team" is marketing);
- **offices** ("hybrid in London", "onsite in New York", "based in our
  Toronto office");
- **work authorization** ("must be authorized to work in the United
  States"), and Work at a Startup's first-party visa field ("US
  citizen/visa only");
- **sponsorship** (offered; offered "but not for every role";
  unavailable), **relocation** (help offered, or required);
- **engagement** (contractors, B2B, employer of record such as Deel or
  Oyster, where; "we don't work with contractors");
- **time zones** (below).

### Time zones

A time-zone requirement is kept separate from geography, with its kind:
**within** a range ("You must be located between UTC-5 and UTC+1", "based
in a European time zone"), **hours** of a zone with an optional tolerance
("EST ±3 hours", "Pacific time"), or **overlap** of so many hours
("4 hours overlap with EST", counted against an 8-hour day). Your zones
are the ones you stated, else your city's, else your country's range.

- Within: inside passes; outside fails for a requirement; partly inside
  (a country spanning zones) is uncertain.
- Hours: within the tolerance (the stated one, else 3 hours) passes;
  outside a stated tolerance, or with no working-day overlap at all,
  fails; otherwise uncertain ("5h from your time zone; the posting doesn't
  say how much shift is acceptable").
- No time-zone language is "no requirement published", not "unrestricted";
  vague language ("some overlap with the team") is uncertain; "flexible
  hours" says hours are flexible. Nothing is inferred from where the
  company's headquarters are.

### Visa, work authorization, contractors and EOR

- **Authorization** is taken only from what you stated: `authorized-in`
  preferences, and "no sponsorship needed", which is read as authorized
  *where you live* (and nowhere else). Living somewhere is not assumed to
  mean you may work there.
- An explicit requirement ("authorized to work in the US", the visa field,
  or an office abroad) passes when you hold it; with sponsorship needed it
  is conditional if the posting offers sponsorship, uncertain if it offers
  it "not for every role" or doesn't say, and ineligible if it doesn't
  sponsor; otherwise it is uncertain and says how to settle it.
- **"No visa sponsorship" is not "no international applicants."** For a
  remote option with no authorization requirement it restricts nothing and
  is reported as such ("doesn't restrict remote work from Brazil by
  itself").
- **"US only, no sponsorship"** is ineligible from Brazil because of "US
  only", not because of "no sponsorship", and it says nothing about a
  contractor path. A contractor or EOR path exists only where the posting
  names one (then it is its own work option), or for "remote worldwide,
  contractor", which is strong evidence. When the scope is unknown, the
  missing contractor information is named as a reason.
- Your `engagement` preferences rule paths out (contractor `avoid`) or
  require them (contractor `require`: a job that doesn't say whether it
  hires contractors is uncertain).

### Eligibility decisions and rules

Each work option goes through these rules, in order (each a function in
[`rules`](crates/jobhunt-eligibility/src/rules.rs)):

1. **Listing** (a gate on top of the decision): verified active,
   recently, at an authoritative source; otherwise "not trusted enough to
   recommend".
2. **Work mode and presence**: a work mode you require; being at an
   office (same city passes; elsewhere in your country is uncertain, or
   conditional if you'd relocate; abroad is ineligible if you won't
   relocate, conditional if you would, uncertain if your profile doesn't
   say).
3. **Countries** the job allows or rules out (including cities).
4. **Regions** and remote scope.
5. **Work authorization and sponsorship.**
6. **Contractor, B2B and EOR** engagement.
7. **Time zones.**
8. **Ambiguity**: what could not be read.

An option is **ineligible** if any rule fails, else **uncertain** if any
rule can't tell, else **conditional** if it holds only on a condition you
haven't ruled out (you relocate; the company grants the sponsorship it
offers), else **eligible**. The job's decision is its best option's; the
other options are listed with theirs. Every reason records the rule, its
conclusion, the posting's words (source and field) and the profile fact it
used, so "why am I eligible?" is answered from the stored decision itself.

**Unknowns stay unknown**: no location in your profile, an unrecognized
place, a remote scope that isn't published, a membership usage disagrees
on, an authorization your profile doesn't state: each is uncertain, with
the reason and, where there is one, the command that settles it.

### Conflicting evidence

Both statements are kept and the disagreement is recorded:

- The structured fields say one scope and the description a narrower one
  (listed "Remote - Worldwide", description "US only"): the narrower,
  explicit restriction applies, and the conflict is shown.
- The description is broader than the fields (listed "Remote (US)",
  description "open to candidates across the Americas"): for someone the
  fields exclude but the description includes, it is uncertain, with both
  statements: the description may describe the company, not the role.
- "Remote", but "applicants must live in NYC": the city requirement
  applies and the conflict is shown.
- Several source records of one opportunity: the live records decide; a
  record that says nothing defers to one that answers; records that
  contradict each other (one eligible, one ineligible) make the answer
  uncertain, with each record's answer kept.

### Compensation verification

Compensation is verified as facts, not scored: whether it is published,
each range with its source label ("Canada Annual Pay Range"), minimum and
maximum where given, pay period, and its **currency evidence**: an ISO code
the source gives, or an ambiguous symbol (`$`, `¥`) kept as such with the
currency unknown. A bare `$` is never read as USD, whatever country the
job is in. Each verification compares with the previous successful one:
first verification, unchanged, changed (with the previous version),
newly published, removed. Currencies are not converted. Whether pay meets
your minimum is a ranking question and is not part of eligibility.

### Caching and revisions

Eligibility decisions are stored in `eligibility_decisions` under a key
that is a digest of everything they depend on: the profile id and
revision, every source record's material content and lifecycle status, the
verification each record rests on, and the rules revision
(`RULES_VERSION`, bumped with any rule or normalization change). A changed
preference, an UPDATED job, a new verification or a rule change is a new
key, so a stored decision can never outlive its inputs; older rows remain
as an audit trail. Trust is always recomputed, since it depends on the
clock.

### Browser automation

Not used. Every supported family publishes what verification needs as
plain HTTP responses (JSON APIs, or server-rendered page data), so no
browser runs anywhere. A browser-backed verifier would be warranted for an
important first-party page that shows its listing only after JavaScript
runs (Ashby's hosted application form is one, but its API publishes the
same route, so it is not needed). It would be another implementation of
the `ListingVerifier` trait, isolated from the domain.

### Architecture

```text
jobhunt-cli ──► jobhunt-sources::verify (HttpVerifier) ──► jobhunt-jobs::verification (ListingVerifier)
     │                                                         ▲
     ├──► jobhunt-eligibility (rules, decisions, cache) ───────┤ ──► jobhunt-profile
     └──► jobhunt-storage (VerificationRepository, EligibilityRepository)
```

- `jobhunt-jobs::verification` is the verification domain: records,
  states, authority, freshness, trust, compensation facts, the
  `ListingVerifier` fetch contract, `VerificationRepository` and
  `VerificationService`. No HTTP or SQL.
- `jobhunt-sources::verify` implements the contract over HTTP.
- `jobhunt-eligibility` reads jobs and the profile domain (never profile
  tables), and owns normalization, extraction, rules, decisions and the
  `EligibilityRepository` boundary. No HTTP or SQL.
- `jobhunt-storage` implements both repositories on SQLite.

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
  jobhunt-profile   The profile domain: career profile, evidence claims and
                    policy, preferences and their statement parser, resume
                    re-import rules, export format, ProfileRepository,
                    ProfileService.
  jobhunt-resume    Resume files (PDF, text, Markdown) into text, and the
                    deterministic parser into the profile's ParsedResume.
  jobhunt-jobs      The jobs domain: canonical model, lifecycle rules,
                    cross-source identity, the JobRepository boundary, the
                    Discovery pipeline, and verification (records, states,
                    authority, freshness, trust, the ListingVerifier contract,
                    VerificationRepository, VerificationService).
  jobhunt-eligibility Location normalization (geography and time-zone
                    tables, region definitions), restriction extraction with
                    evidence, profile facts, the rules, decisions and reasons,
                    opportunity aggregation, and the decision cache boundary.
  jobhunt-sources   Adapters (Ashby, Greenhouse, Lever, YC), careers-page board
                    detection, the HTTP verifiers, and the shared HTTP client.
  jobhunt-storage   Storage backends. SQLite today (jobs, profiles,
                    verifications, eligibility decisions).
  jobhunt-cli       The `jobhunt` binary: config, logging, find, show, verify,
                    check, init, profile, claims, preferences, output.
  jobhunt-mcp       Placeholder for the MCP server; intentionally empty for now.
```

Dependencies only point downward: `cli → sources, storage → jobs → core`,
`cli → resume, storage → profile → core`, and `cli → eligibility → jobs,
profile`. `jobhunt-jobs` and `jobhunt-profile` do not depend on any HTTP,
SQL or PDF crate, nor on each other; `jobhunt-eligibility` is the only
place where they meet.

## Tests

```bash
./scripts/check.sh                  # the full local quality gate (what CI requires)
cargo test                          # everything offline
cargo test -p jobhunt-sources --test ashby_live -- --ignored --nocapture
cargo test -p jobhunt-sources --test greenhouse_live -- --ignored --nocapture
cargo test -p jobhunt-sources --test lever_live -- --ignored --nocapture
cargo test -p jobhunt-sources --test yc_live -- --ignored --nocapture
cargo test -p jobhunt-eligibility --test verification_live -- --ignored --nocapture --test-threads 1
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
  database, and a jobs-only database to profiles), lifecycle persistence
  and history, scans, opportunities, search; profiles stored and loaded
  exactly, revision conflicts, deletions, skill evidence, claim queries,
  profile history.
- `jobhunt-profile`: dates, ids, the evidence policy, vocabularies
  (technologies, domains, roles, ownership), the preference statement
  parser, and use cases against an in-memory repository: first import,
  identical re-import, a changed resume (decisions, edits, stale and
  superseded claims, corrected titles), rejection across re-imports,
  manual claims, preferences and supersession, export round trip and
  all-or-nothing import, concurrent writers.
- `jobhunt-resume`: fixtures under `tests/fixtures/` (see its README): a
  two-page Chromium PDF, a TeX-style PDF without space characters,
  Markdown and plain-text resumes, and blank, truncated and fake PDFs.
- `jobhunt-jobs` (verification): active, changed, reopened and deleted
  listings through the lifecycle, timeouts, 5xx and unreadable answers
  keeping the last success, reuse and forced verification, mismatched
  records, unsupported sources, compensation changes across
  verifications, a bare `$` staying ambiguous, the employer page in the
  authority chain, and opportunity trust (one source dead and one live,
  two live, stale, failed, never verified, secondary only, closed by
  discovery after a verification).
- `jobhunt-eligibility`: the rule matrix on synthetic postings (remote
  scopes for Brazil and other countries, disputed memberships, description
  restrictions and preferences, exclusions, on-site and hybrid with and
  without relocation, offices named in prose, mixed options, time zones of
  every kind, work authorization and sponsorship, "no sponsorship" on
  remote jobs, contractors and EOR, relocation, conflicting evidence,
  missing profile facts, office-city scopes); the region definitions at
  their boundaries; every adapter's saved real postings (Linear's Europe
  listing with a broader description, Figma, Spotify, Work at a Startup's
  visa field, Pacific hours, Anthropic's Sydney office, a hybrid job with
  a remote flag); opportunity aggregation and the decision cache
  (profile, job, verification and rules changes each invalidate it).
- `jobhunt-sources` (verification): each family's verifier against a local
  mock serving saved real responses: active, closed and redirected
  listings, working and broken application paths, 5xx, garbage, timeouts,
  one request per Ashby board, unsupported sources.
- `jobhunt-sources`: per adapter, conversion of saved real responses
  (`tests/fixtures/<family>/`, including files with deliberately broken
  records) and HTTP behavior against a local mock server (conditional
  requests, 404s, retries, truncated listings, garbage bodies, failed YC
  detail pages).
- `jobhunt-cli`: config, output, and end-to-end tests: every family served
  from one mock server through one pipeline into a SQLite file, then mutated
  across runs to prove NEW / UNCHANGED / UPDATED / CLOSED / REOPENED, that
  failed and partial scans close nothing, and cross-source grouping; and the
  `jobhunt` binary through the profile flow (`init`, `profile`,
  `preferences`, `claims`, edits, `export`, re-import of a changed PDF,
  `import` into a fresh database, unreadable files) with a temporary
  database; eligibility through the binary (`find` verdicts and
  `--eligible`, `check` with evidence, `show`) over real Ashby and
  Greenhouse responses discovered into a temporary database; and the whole
  verification flow offline: `init` a resume PDF, discover Ashby,
  Greenhouse and Lever fixtures, then `verify` against mock authoritative
  endpoints (`JOBHUNT_VERIFY_ENDPOINT`), `show` and `check` without
  fetching, reuse within the window, `--force`, `--details`, a profile
  change, 503s keeping the last success, jobs taken down, and `find`
  filtering.

Only the offline suite runs in required CI. The live tests run separately,
three times a week and on demand, in the "Live sources" workflow, so a
third-party outage never blocks a merge.

Before sending changes, run `./scripts/check.sh`. It runs formatting,
clippy (warnings denied), the build, every offline test and rustdoc
(warnings denied), exactly as required CI does, and `--msrv` adds the
minimum-Rust check. [CONTRIBUTING.md](CONTRIBUTING.md) describes the CI
checks, the live validation workflow, the PR workflow and branch
protection.

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

Profile:

- Resume parsing is rule-based. Unusual layouts (two-column designs whose
  columns interleave in the PDF, tables, headings JobHunt does not know)
  can be misread; `init` reports what it did not understand, and anything
  can be corrected. Scanned (image-only) PDFs need OCR, which JobHunt does
  not do; `.docx` must be saved as PDF or text first.
- Domain, role and seniority inferences come from fixed vocabularies
  (English, with some Portuguese). They are always marked inferred and
  need your confirmation.
- The preference parser understands common phrasings in English;
  everything else is kept as written and flagged.

Eligibility:

- Places are read from a table of about 75 countries, their business
  regions, subdivisions used in postings, and major tech cities; anything
  else stays unrecognized (and the answer `unknown`). Region membership is
  a judgment ("North America" includes Mexico only *maybe*).
- Description sentences are read with fixed English cues ("based in",
  "authorized to work", "sponsor", time zones); other phrasings and
  languages are missed, so a missing restriction is not proof there is
  none. Each answer shows the sentences it used.
- A remote job in a city ("Remote (San Francisco; Oakland)") is treated as
  commuting distance, and whether elsewhere in the country works is
  uncertain. Commuting distance between two cities is never estimated.
- Work authorization is only what you stated; JobHunt does not know
  citizenship rules, and the answer is a compatibility signal, not legal
  advice.
- No currency conversion, and pay is not part of eligibility (a pay
  minimum is a ranking question).
- Verification is plain HTTP. Employer careers pages are not checked
  (so no listing reaches `employer_first_party` yet); Ashby application
  pages render in a browser and are known from the API, not requested; a
  Work at a Startup application page may need an account.
- A job verified closed stays open in the jobs table until discovery
  closes it; meanwhile `show`, `check` and `verify` show it closed, `find`
  marks it ("Verified closed at its source") and `--eligible` /
  `--possible` never offer it.

Jobs:

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
- Greenhouse boards hosted in its EU region (`job-boards.eu.greenhouse.io`)
  are recognized in URLs but read through the global API, which does not
  serve them. Lever's EU region is supported (`region = "eu"`) but was not
  verified against a live EU site. Only public, unauthenticated endpoints
  are used.
