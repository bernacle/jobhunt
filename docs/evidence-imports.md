# Evidence imports: resume, LinkedIn export, GitHub

Narrow builds one career profile from several sources. A resume, a
LinkedIn data export and a public GitHub account all feed the **same**
evidence graph (`jobhunt_profile`): the same experiences, projects,
education, skills and claims, the same evidence policy, the same review
queue. There is no second profile model.

> Imported data is evidence, not unquestionable truth. Evidence from
> different sources converges into one graph. The person stays in control
> of what becomes confirmed profile knowledge.

- [Invariants this builds on](#invariants-this-builds-on)
- [Sources](#sources)
- [LinkedIn data export](#linkedin-data-export)
- [GitHub](#github)
- [One graph, several sources](#one-graph-several-sources)
- [Re-import and idempotency](#re-import-and-idempotency)
- [Removing a source](#removing-a-source)
- [Confirmation semantics](#confirmation-semantics)
- [Privacy](#privacy)
- [Storage](#storage)
- [Commands, API and web](#commands-api-and-web)
- [Known limitations](#known-limitations)

## Invariants this builds on

These held before LinkedIn and GitHub imports existed (resume import only,
`crates/jobhunt-profile/src/import.rs`), and still hold:

1. **Records** (experiences, projects, education, skills) carry a
   `RecordMeta`: an `origin` (`resume` or `user`), the `source` they were
   read from (document + verbatim snippet + section), an `import_key`
   identifying them across re-imports, the person's `verification`,
   `stale_since`, and `edited_fields` a re-import never overwrites.
2. **Claims** are keyed by *what they are about and what they say*: the
   subject's id plus the bullet's normalized words, or the technology,
   domain or role topic (`exp_…|technology|rust`). The key does not
   depend on the document, so the same statement found again is the same
   claim.
3. **The evidence policy** (`Claim::standing`) decides what may be used:
   rejected claims never; a claim whose source disappeared needs review
   even if it was confirmed (until confirmed again); confirmed and
   user-entered claims are usable; claims quoted verbatim with high
   confidence are usable ("grounded"); inferences and uncertain readings
   need review. `ProfileData::standing` also rejects claims about rejected
   records.
4. **Decisions survive re-imports.** Confirmed stays confirmed, rejected
   stays rejected (never resurrected as trusted). A confirmation is reset
   only when the statement of a one-per-record claim (title and period,
   degree, project) changed.
5. **Nothing imported is deleted by a re-import.** What a new import no
   longer contains is marked stale. What the person entered is never
   touched by an import; fields they edited are never overwritten.
6. **Removing** a record the person created deletes it; removing an
   imported record rejects it (kept, hidden, not brought back).
7. **Ranking** reads non-rejected evidence (skills by evidence strength,
   domains, role signals); **application context** reads only usable
   claims. Neither knows where a claim came from.
8. **Storage** is the whole aggregate saved atomically under an
   optimistic revision (SQLite relationally, Postgres as sealed JSON
   entities); SQLite migrations are additive.

## Sources

`Origin` says which source created a record: `resume`, `linkedin`,
`github` or `user`. Each imported source is a `SourceDocument` whose
`kind` says what it is (`pdf`/`text`/`markdown` for resumes, `linkedin`,
`github`) and whose text is what Narrow read (snippets point into it):

| Source | Document | Identity |
| --- | --- | --- |
| Resume | one per file (by SHA-256) | the latest one is "the resume" |
| LinkedIn export | one per profile, updated in place | career rows only, rendered as text |
| GitHub | one per profile (one account), updated in place | the normalized public snapshot |

## LinkedIn data export

The person downloads their own data from LinkedIn (*Settings → Data
privacy → Get a copy of your data*) and gives Narrow the file. Narrow never
scrapes LinkedIn, never asks for a password or cookie, and uses no
LinkedIn API.

Accepted: the `.zip` LinkedIn sends (Basic or Complete export), the folder
it unpacks to, or one of its CSV files. Narrow opens only these files, by
name, and never the rest of the archive:

| File | Read | Becomes |
| --- | --- | --- |
| `Profile.csv` | Headline, Summary, Geo Location, Websites | headline/summary/location when the profile has none; websites as claims |
| `Positions.csv` | Company Name, Title, Description, Location, Started On, Finished On | experiences, employment claims, description sentences, technologies, inferred domains/roles |
| `Education.csv` | School Name, Degree Name, Start Date, End Date | education |
| `Skills.csv` | Name | skill claims |
| `Certifications.csv` | Name, Authority, Started On, Finished On | certification claims |
| `Projects.csv` | Title, Description, Url, Started On, Finished On | projects |
| `Languages.csv` | Name, Proficiency | spoken languages when the profile has none |

Everything else (messages, connections, invitations, contacts, email
addresses, phone numbers, ads, search history, recommendations, …) is never
opened. Missing categories are not negative evidence: an export without
`Certifications.csv` says nothing about certifications.

A file Narrow cannot read truthfully fails the whole import with a clear
message and changes nothing: not a ZIP/CSV, no recognized career file, a
recognized file without the columns Narrow expects, or malformed CSV.

## GitHub

Narrow reads a **public** GitHub account through GitHub's official REST
API (`api.github.com`): the user, their public repositories (paginated),
and their public organization memberships; with a token, also the
language breakdown of the repositories it keeps. No HTML scraping, no
private data. A token is optional (no scopes are needed; `GITHUB_TOKEN` for
the CLI, `JOBHUNT_GITHUB_TOKEN` for the hosted server) and is never stored.

Requests: without a token about three (GitHub allows 60 unauthenticated
requests an hour), each repository's primary language standing for its
code; with one, up to one more per kept repository, four at a time. An
unknown account, an organization account, a rate limit or a repository
list that cannot be read completely fail the import clearly (a partial
list would make missing repositories look deleted). A repository whose
statistics fail keeps its primary language, and the import reports it.
A profile follows one account: importing another is refused until the
first is removed.

Which repositories become evidence:

- **kept:** public repositories the account owns, most recently pushed
  first (at most 30), archived ones marked as archived;
- **skipped:** forks (someone else's work), empty repositories, the
  profile README repository (`login/login`), repositories owned by
  organizations or other people (ownership is ambiguous), private ones.

What each kept repository becomes (a project):

| Claim | Provenance | Example |
| --- | --- | --- |
| the repository | extracted (fact) | "Owns the public GitHub repository alice/lox (archived)" |
| each main language (≥ 15% of its code, at most 3; the primary language without statistics) | extracted (fact) | "Rust code in alice/lox (GitHub)" |
| technologies named in its description or topics | extracted, medium confidence (review) | "alice/lox's description or topics mention Kubernetes" |
| domains its description and topics suggest | inferred (review) | "Public project about payments: alice/lox" |

And, for the profile as a whole, one **inference per language** with
recent activity (a repository pushed in the last 12 months):
"Recent hands-on Rust work in public GitHub repositories", with the
repositories as its basis. Inferences need the person's review.

What GitHub is **not** read as: stars are context in the snippet, never
quality; organization membership is kept in the snapshot as evidence,
never employment; language share is never a skill level. No GitHub claim
says "expert", "experienced", "senior" or anything like it.

## One graph, several sources

Records and claims keep one `source` plus `corroborations`: the other
sources that contain the same thing. A resume and a LinkedIn export that
both list "Senior Software Engineer at Acme" produce **one** experience
and **one** employment claim with two sources.

Matching across sources is conservative:

- **experiences:** same company and same title (normalized), and periods
  that overlap (unknown dates do not conflict); exactly one candidate.
- **projects:** same URL, or same name with exactly one candidate.
- **education:** same institution and same degree.
- **claims:** the same key (same subject, same statement or topic).

When matching is ambiguous (several candidates), the records stay
separate and the import notes it. When the same position disagrees (same
company and title, different dates), the record is shared but the other
source's statement becomes its own claim ("LinkedIn: … (Jan 2019 –
Present)") with medium confidence and the difference as its basis, so it
waits in the review queue. A different title at the same company is a
different position until the person says otherwise.

There is no source precedence. The source that created a record owns its
fields (a re-import of that source updates them); other sources only add
evidence. Fields the person edited are never overwritten by anyone.
Records the person added are matched too (LinkedIn/GitHub add evidence to
them) but never changed.

## Re-import and idempotency

Importing the same export or account again changes nothing: same
document, same records, same claim ids, same decisions. When a source
changes:

- what it still contains is refreshed (for records it created) or keeps
  its corroboration;
- what it no longer contains loses that source; if no other source
  supports it any more it becomes stale (never deleted), and stale
  claims need review before they are used again, as with resumes;
- confirmed claims stay confirmed, rejected ones stay rejected and are
  never resurrected, edits are kept.

## Removing a source

`narrow profile remove-source linkedin|github` (and the web's Remove
button) takes a source out:

- its document (the text Narrow kept) is deleted;
- records and claims that other sources also support stay, and their
  provenance moves to the remaining source;
- records and claims only this source supported are deleted, unless the
  person decided about them (confirmed, rejected, edited): those are kept
  without a source, stale, so a confirmation needs renewing before use
  and a rejection keeps protecting against a later re-import;
- what the person entered is never touched.

Resumes are not removed this way; import an updated resume instead.

## Confirmation semantics

| What | Standing |
| --- | --- |
| LinkedIn position, degree, skill, certification (structured, verbatim) | usable, "quoted from your LinkedIn export"; reviewable like resume facts |
| LinkedIn position that conflicts with another source | needs review |
| GitHub repository, its main languages | usable facts about public code, "read from GitHub" |
| "Recent hands-on Rust work…" | inferred: needs review |
| domains, roles, seniority inferred from any source | inferred: needs review |
| a claim several sources support | one claim; every source shown behind the evidence disclosure |

## Privacy

- LinkedIn: only the career files above are opened; within them only the
  columns listed. Names, birth date, address, emails, phone numbers,
  license numbers and free-text notes are not read.
- GitHub: only public data; the snapshot keeps the login, profile URL,
  public counts, organization logins and per-repository facts, not
  e-mail addresses.
- Nothing from a file or the API is logged; errors name files and
  columns, never contents.
- Tests use fictional fixtures only.

## Storage

Additive only (`20261004000000_evidence_sources.sql`). SQLite gains
columns: `corroborations` (JSON) on experiences, projects, education,
skills and claims; `import_origin` on those records; `source_kind` on
documents. Its `CHECK` constraints on `origin` and `kind` cannot be
widened without rebuilding tables other tables reference, so the legacy
columns keep a value they accept (`resume`, `text`) and the new column
says which source it really is. Existing rows read exactly as before.
Postgres stores profile entities as sealed JSON and needs no migration.
The export format stays version 1: a profile without the new sources
writes the same file; one with them uses optional additions older
versions refuse with a schema error.

## Commands, API and web

| Where | How |
| --- | --- |
| CLI | `narrow profile import-linkedin <export.zip\|folder\|file.csv>`, `narrow profile import-github [user]`, `narrow profile remove-source linkedin\|github` |
| API | `PUT /api/v1/profile/linkedin`, `POST /api/v1/profile/github`, `DELETE /api/v1/profile/sources/{source}` |
| Web | Profile → Sources: import a LinkedIn export, import from GitHub, remove a source |

After an import, new claims wait in the existing review queue (`narrow
claims review`, Profile → Review).

## Known limitations

- LinkedIn exports are parsed from the columns LinkedIn used when this was
  written (2026); a changed layout fails clearly rather than guessing.
  Field of study is not a separate column (it stays inside the degree).
- LinkedIn fills the headline, summary, location and spoken languages
  only when the profile has none; removing the LinkedIn source does not
  clear those basics (edit them with `narrow profile edit basics`).
- Web uploads are limited to 16 MB (the API's body limit); the CLI reads
  any size from disk, opening only the career files.
- GitHub: only repositories the account owns. Contributions to others'
  repositories, organization repositories and README contents are not
  read; repositories generated from a template are not told apart from
  others (the list API does not say); at most the 30 most recently pushed
  repositories are used.
- GitHub responses are not cached between imports (no ETag store for the
  profile side); an import is a deterministic manual re-read.
- One GitHub account per profile.
- No MCP tool imports these sources yet; the CLI, the API and the web do.
