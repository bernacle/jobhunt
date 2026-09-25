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

`jobhunt init resume.pdf` builds your career profile from your resume
(see [Career profile](#career-profile)): experiences, projects, education,
skills, domains, role signals, and an evidence graph where every claim keeps
the resume text it came from and your decision about it. `jobhunt
preferences add "…"` records what you want in your own words.

Not built yet: eligibility, ranking and match scores, preference learning
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
the older one, which is kept as history. Nothing here decides whether a job
fits; eligibility and ranking will read these constraints later.

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
                    cross-source identity, the JobRepository boundary, and the
                    Discovery pipeline.
  jobhunt-sources   Adapters (Ashby, Greenhouse, Lever, YC), careers-page board
                    detection, and the shared HTTP client.
  jobhunt-storage   Storage backends. SQLite today (jobs and profiles).
  jobhunt-cli       The `jobhunt` binary: config, logging, find, show, init,
                    profile, claims, preferences, output.
  jobhunt-mcp       Placeholder for the MCP server; intentionally empty for now.
```

Dependencies only point downward: `cli → sources, storage → jobs → core`
and `cli → resume, storage → profile → core`. `jobhunt-jobs` and
`jobhunt-profile` do not depend on any HTTP, SQL or PDF crate, nor on each
other.

## Tests

```bash
./scripts/check.sh                  # the full local quality gate (what CI requires)
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
  database.

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
- Location and remote data is kept as each source states it; it is not
  geocoded or normalized into countries, so eligibility filtering is future
  work.
- Greenhouse boards hosted in its EU region (`job-boards.eu.greenhouse.io`)
  are recognized in URLs but read through the global API, which does not
  serve them. Lever's EU region is supported (`region = "eu"`) but was not
  verified against a live EU site. Only public, unauthenticated endpoints
  are used.
