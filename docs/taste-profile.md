# The candidate taste profile

What Narrow understands about the kind of role and company a person would
genuinely want, kept apart from what they can practically take (BRU-321).
It replaces the long Preferences form as the default experience: the person
says in a few words what they're looking for, Narrow reads it into a short
summary, and they confirm or correct it. It is what ranking's fit reads
(BRU-322; see [What ranking reads](#what-ranking-reads)).

- Domain: [`crates/jobhunt-profile/src/taste`](../crates/jobhunt-profile/src/taste)
- Use cases and views: [`crates/jobhunt-app/src/taste_profile.rs`](../crates/jobhunt-app/src/taste_profile.rs)
- Model providers: [`crates/jobhunt-ai`](../crates/jobhunt-ai)

## Two questions, two models

| | Taste | Practical constraints |
| --- | --- | --- |
| Answers | What would this developer genuinely want? | Can they realistically pursue this job? |
| Examples | small technical teams, high ownership, backend/platform, startups; avoids early-career roles and process-heavy large companies | remote only, based in Brazil, authorized in Brazil, not relocating, time zones, a pay floor |
| Nature | semantic, often fuzzy; every statement has provenance, a confidence and the person's review | deterministic and explicit |
| Stored as | `TasteAssertion`s and the `TasteBrief` | the existing structured `Preference` records |
| Read by | ranking's fit (BRU-322) | eligibility and ranking's practicality |

Compensation is never taste: a pay floor or target is a practical
constraint, unknown pay stays neutral, and a model's statement mentioning
pay, visas, relocation or time zones is dropped from taste (and noted as a
constraint the words mentioned).

## The taste model

One statement, `TasteAssertion`:

| Field | |
| --- | --- |
| `dimension` | `seniority`, `work_shape`, `specialization`, `ownership`, `company`, `team`, `culture`, `domain`, `technology`, `work_style`, `other` |
| `value` | a canonical token (`senior`, `database_internals`, `small_team`, `process_heavy`) or, for anything Narrow has no token for, the words normalized |
| `polarity` | `prefer`, `open` (fine, not sought), `avoid`, `neutral` (said not to matter; kept so it is never inferred again) |
| `text` | what the person sees: a short phrase ("Small technical teams") |
| `confidence` | `low`, `medium`, `high` (qualitative; inferences are never `high`) |
| `origin` | `stated` (the person wrote it), `interpreted` (Narrow's reading of their words), `profile` (inferred from confirmed career evidence), `learned` (a feedback pattern), `legacy` (a structured preference set earlier) |
| `review` | `unreviewed`, `confirmed`, `corrected`, `removed` |
| `sources` | the words quoted verbatim, the preference, the evidence (with record ids), the feedback pattern, or the person |
| `original` | what Narrow had read before a correction |

Canonical values per dimension (open-ended: nothing forces every person
into every dimension, and unknown values are kept as words):

- `seniority`: `early_career`, `mid`, `senior`, `staff_plus`. A range is
  several statements (prefer `senior`, open to `staff_plus`, avoid
  `early_career`). A current title is a medium-confidence starting point,
  never the only level someone would take.
- `work_shape`: `backend`, `platform`, `infrastructure`, `product`,
  `full_stack`, `frontend`, `mobile`, `database_internals`,
  `distributed_systems`, `developer_tooling`, `security`, `ml_product`,
  `ml_research`, `research`, `data`, `sre`, `embedded`. Work shape is not
  technology: PostgreSQL experience is not wanting to build storage engines.
- `specialization`: `broad`, `moderate`, `deep`.
- `ownership`: `high`.
- `company`: `startup`, `early_stage`, `growth`, `established`,
  `small_company`, `large_company`, `founder_led`, `product_company`,
  `agency`, `consulting`, `public_company`, `open_source`.
- `team`: `small_team`, `large_team`, `distributed` (a company's size never
  stands in for a team's).
- `culture`: `strong_engineering`, `process_heavy`, `fast_paced`,
  `mentorship`, `remote_first`.
- `work_style`: `individual_contributor`, `management`, `greenfield`,
  `maintenance`, `async_communication`, `meetings`, `product_closeness`,
  `on_call`.

`TasteBrief` holds the person's words ("What kind of job are you looking
for?"), verbatim, and the last interpretation: which interpreter, whether
it fell back, the digest of what it was given, a one-sentence summary,
ambiguities, and practical constraints the words mentioned.

### Composition and precedence

The profile the person sees (and BRU-322 reads) is composed at read time
(`taste::compose`) from the stored statements, the active structured
preferences (read live) and the active learned patterns (read live). For
one `dimension:value` the first present wins, and the rest add their
sources when they agree or are kept as `against` when they don't:

1. a statement the person wrote, confirmed, corrected or removed (a removal
   hides the key; the key a correction replaced never comes back);
2. Narrow's reading of their words;
3. a structured preference set earlier;
4. an inference from the profile or from feedback;
5. a learned pattern (shown in "Learned over time", never as something the
   person said, and not "firm" until they confirm it).

## Provenance and wording

What the person sees is short; where each line comes from is one tap away
("How Narrow read this"). Each statement says, in a few words:

| Basis | When |
| --- | --- |
| You said | they wrote it (a correction or an added sentence) |
| You confirmed / You corrected this | they reviewed Narrow's reading |
| Narrow's reading of your words | read from their words, not yet reviewed (the summary says "Narrow's reading · check it") |
| Inferred from your profile | from confirmed career evidence, e.g. the latest title |
| From your earlier settings | a structured preference |
| Learned from your feedback | a pattern from saves, rejections, applications |

So "You said you prefer small teams" and "Narrow inferred you prefer small
teams because you saved two startup jobs" never read the same.

## Corrections and confirmation

The person's decisions are authoritative (`taste::edit`):

- **Looks right** confirms every statement of the summary Narrow read.
- **Change** takes new words and/or a polarity. New words are read (just
  that sentence, with the dimension as a hint) into structured values; the
  corrected statement is the person's (`stated`, `corrected`) and keeps what
  Narrow had read in `original`. The replaced key never comes back.
- **Doesn't matter** is polarity `neutral`: kept, hidden from the summary,
  never inferred again.
- **Remove** keeps a tombstone, so no reading brings it back.
- **Add one sentence** is the person's (`stated`, `confirmed`).
- A new or changed description, **Read my words again**, a resume,
  LinkedIn or GitHub import, and learned feedback never overwrite any of
  these. Re-reading replaces only Narrow's own unreviewed readings.
- A decision about a statement that came from a structured preference is
  applied to that preference too (removed, or given the new stance), so
  what ranking reads today agrees with what the person sees.

## Reading the words: model or rules

Interpretation is behind `TasteInterpreter`
(`jobhunt_profile::taste::reading`). Two implementations:

- **`rules/1`** (`RulesInterpreter`): built in, deterministic, offline. The
  default, the open-source path, the fallback, and what tests read with.
  It reads a compact vocabulary with clause polarity ("I don't want …",
  "open to …"), keeps what it can't place as an ambiguity, and infers one
  thing from the profile: the latest title's level, at medium confidence,
  only when the words don't mention a level.
- **A model** (`jobhunt_ai::ModelInterpreter`): the Anthropic Messages API
  (structured outputs, `output_config.format` with a JSON schema; default
  model `claude-opus-5-5` at `effort: low`, with server-side refusal
  fallback on the hosted API), or any OpenAI-compatible server
  (`response_format: json_schema`: OpenAI, Ollama, vLLM, LM Studio). Plain
  HTTP, no SDK in the domain.

A model answers in JSON of a fixed schema, never prose. `parse_reading`
validates it strictly: malformed output is rejected whole; a statement with
an unknown dimension, a quote that isn't in the person's words, evidence it
wasn't given, no surviving source, or anything about pay, visas, relocation
or time zones is dropped and counted. Transient failures (connection,
timeout, 408/409/429/5xx) and garbled answers are retried with backoff;
refusals, truncation and other client errors are not. If the model still
fails, the rules read instead and the interpretation says so
(`outcome: fallback`, with a note); the next description or "read again"
tries the model again.

### When a model is called

The taste reader is called only on: a new or changed description, an
explicit "read again", and a correction with new words or an added sentence
(that sentence alone). Never on a page load, a ranking, Today, or a job.
The same input (a digest of everything the interpreter is given, and its
name) is never interpreted twice unless the person asks. (The separate fit
reviewer of BRU-322, off by default, reads a ranking's shortlist: see
[fit-and-practicality.md](fit-and-practicality.md#semantic-review).)

### Exactly what is sent

`TasteRequest::build` is the only place a request is assembled; `render`
turns it into the text a model reads, after the fixed system prompt:

- the person's words about what they want (the description, or one
  correction/added sentence);
- for each of up to 8 visible experiences, most recent first: the title, the
  years (`2021–present`), the employment kind, and the topics of *usable*
  role, domain and ownership claims and demonstrated technologies (up to 8);
- active learned patterns: polarity, the phrase, and the basis ("2 saves
  and 1 reason in your words"); never pay, never a particular employer;
- what the person already settled (their statements and removals), so a
  reading doesn't contradict them.

Never sent: name, email, phone, contacts, location, employers' names,
experience summaries, education, resume or LinkedIn text, GitHub account,
job ids, feedback reasons verbatim. Prompts and answers are never logged:
logs carry the provider, model, attempt, duration and counts only; errors
never carry the answer.

### Configuration

Local (`config.toml`; keys never go in the file):

```toml
[ai]
provider = "anthropic"        # or "openai" (any OpenAI-compatible server)
model = "claude-opus-5-5"     # OpenAI-compatible servers must name one
base_url = "http://localhost:11434/v1"   # optional (proxy, self-hosted)
api_key_env = "ANTHROPIC_API_KEY"        # the variable holding the key
timeout_secs = 40
```

Cloud: `JOBHUNT_AI_PROVIDER`, `JOBHUNT_AI_MODEL`, `JOBHUNT_AI_BASE_URL`,
`JOBHUNT_AI_API_KEY` (secret), `JOBHUNT_AI_TIMEOUT_SECS`. A configuration
that can't be used (a missing key) never stops Narrow: the built-in reader
is used, and the Preferences page says why. `narrow doctor` (cloud) shows
the setting.

## Migration

Nothing is migrated destructively, and reading the profile writes nothing:

- company and team kinds, roles, domains and work styles set before map
  live onto taste (`small_team` → team: small teams; `small_company` →
  company: small companies; `early_stage` → company: early-stage; a role
  "senior platform" → seniority senior + work shape platform; a "must have"
  says so), shown as "From your earlier settings";
- work setup, location, authorization, relocation, time zones, sponsorship,
  engagement, pay floor and target, and the unknown-pay/unclear-eligibility
  policies are practical constraints, listed as such;
- the words in "In your words" are the starting point ("What you told
  Narrow before"); interpreting them makes them the description, verbatim;
- the structured records stay exactly as they were, and ranking keeps
  reading them. Every structured setting is still editable: practical ones
  under "Edit constraints", the rest under "Fine-tune".

A new description replaces the previous description's statement (and what
its words set); statements added before the taste profile are left alone.

## Storage, sync and export

- SQLite: `profile_taste` and `profile_taste_briefs` (additive migration
  `20261006000000_taste_profile`); the record is a JSON body beside
  inspectable columns.
- Postgres: two new profile entity kinds, `taste` and `taste_brief`,
  encrypted and versioned like every other (the migration widens the kind
  check). The storage contract runs the same round trip on both.
- Sync: entity-level, like every record. An older client pulling a profile
  that has a taste profile fails to read the new kinds with an error (as it
  did for LinkedIn/GitHub sources) until it is updated; nothing is lost.
- Export: profile format version **3** when there is a taste profile
  (`taste_brief`, `taste`, with provenance, corrections and tombstones); a
  profile without one still writes v2 or v1 exactly as before. Older
  versions refuse a v3 file with a clear message rather than dropping the
  corrections. The state export carries it inside the profile.

## Interfaces

| Where | What |
| --- | --- |
| Web | Preferences: What you're looking for, What Narrow understands (Looks right, Edit), Practical constraints (Edit constraints), Learned over time, Fine-tune. Onboarding: one question, then the summary |
| API | `GET /api/v1/taste/profile` → `TasteProfileView`; `POST /api/v1/taste/profile` with a `TasteAction` (`describe`, `reinterpret`, `confirm`, `correct`, `neutral`, `remove`, `add`) → `TasteUpdateResult` |
| MCP | `get_taste_profile`, `update_taste_profile` |
| CLI | `narrow preferences` (the summary and constraints; `--all` adds every setting), `describe "…"`, `confirm [ids]`, `correct <taste_…> ["…"] [--polarity …]`, `reinterpret`, `remove <taste_…>` |

## What ranking reads

Since BRU-322, ranking's fit ([fit-and-practicality.md](fit-and-practicality.md))
reads the composed profile, every statement weighed by whose it is:

```rust
let (data, profile, learned) = app.candidate_taste().await?;   // jobhunt_app
// or, from a ProfileData and learned signals:
let learned = jobhunt_ranking::taste::learned_signals(&taste_model);
let profile = jobhunt_profile::taste::compose(&data, &learned);
```

| The statement | Counts as | A job going against it |
| --- | --- | --- |
| the person's (written, confirmed, corrected), or an earlier setting they entered | firm (1) | a material contradiction |
| Narrow's reading of their words, an inference (medium confidence or better) | soft (0.6) | a material contradiction |
| a low-confidence reading | weak (0.3) | holds a strong fit back |
| a learned pattern, never confirmed | learned (0.3) | holds it back when established with high confidence, else only said |
| neutral ("doesn't matter") | nothing | nothing |
| removed (a tombstone) | nothing | nothing |

`RankingService` composes the profile on every ranking (so structured
preferences set before the taste profile keep working and nobody redoes
onboarding), and the ranking key covers it through the profile revision and
the learned-taste digest. The benchmark's candidates carry their `[taste]`
profile in `candidate.toml`, loaded into the profile the ranker is given.
