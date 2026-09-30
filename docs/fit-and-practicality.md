# Fit first, practicality second (BRU-322)

Narrow answers one question on Today:

> Which companies and roles are unusually right for this developer?

not "which jobs satisfy enough filters to reach a score". Every job is
assessed on two separate questions:

| | Fit | Practicality |
| --- | --- | --- |
| Asks | Would this person genuinely want this company and role? | Can they realistically pursue it, and what still needs checking? |
| Decides | the recommendation (the tier) | what rules it out, known concerns, things to check |
| Reads | the composed taste profile (BRU-321), career evidence, the posting's work | eligibility, verification, pay, work setup |
| Code | [`jobhunt_ranking::fit`](../crates/jobhunt-ranking/src/fit.rs) | [`jobhunt_ranking::practicality`](../crates/jobhunt-ranking/src/practicality.rs) |

Fit creates the recommendation. Practicality can make it impossible, add a
material concern, add something to check, or stay unknown. **Practicality
never creates fit**: pay, remote work, verification and freshness add
nothing to it, and what a posting doesn't say (pay, team size, stage)
never lowers it.

- Benchmark and gate: [recommendation-benchmark.md](recommendation-benchmark.md)
- What must hold: [ranking-invariants.md](ranking-invariants.md)
- The taste model: [taste-profile.md](taste-profile.md)

## The pipeline

```text
open jobs
  │ 1. hard impossibility (eligibility, work setup, relocation, a verified pay floor)   deterministic
  │ 2. reading the work (level, shape, depth, company, team, culture)                  deterministic, cached per posting version
  │ 3. fit assessment against the composed taste profile                               deterministic
  │ 4. contradictions: material → poor; holding → at most plausible                     deterministic
  │ 5. semantic review of the shortlist (optional model, cached, bounded)               model, validated
  │ 6. practicality annotation (blockers, concerns, things to check, facts)             deterministic
  ▼ 7. Today: strong fits only, one per company, no quota
```

1. **Hard impossibility** reuses the eligibility engine
   ([`jobhunt_eligibility`](../crates/jobhunt-eligibility)); ranking never
   re-reads geography. BRU-322 fixed the readings BRU-320 exposed:
   "Remote in United States" (and "in North America", "within Canada") is a
   remote scope, and "open to US-based candidates" limits who can apply
   (eligibility `RULES_VERSION` 5).
2. **Reading the work** ([`facets::work`](../crates/jobhunt-ranking/src/facets/work.rs)):
   - *level*: the title's ("Senior", "Staff+", "Early Career", "New Grad"),
     else the description's ("0-2 years", "our new-grad program",
     "6+ years of experience");
   - *shape*: the title's own role words first. "Backend Engineer,
     Payments Infrastructure" is backend work on a team called Payments
     Infrastructure; "Senior Software Engineer, Platform" is platform work;
     the description decides only for a title that names no role;
   - *depth*: specialized work that shares vocabulary with ordinary
     engineering but is a different job: database-engine internals,
     Kubernetes control-plane internals, model training, low-latency
     trading, privacy engineering, security research. A title cue settles
     it; in a description it takes two different cues, and a denied cue
     ("no machine learning background needed") is none. A specialized role
     is its specialty: the description's generic "backend" doesn't count;
   - *company, team, culture*: stage and size as stated ("a 15-person
     startup", "a Series C company of about 400 people", "a Fortune 500
     insurer"), the team someone joins ("a team of 60 engineers in the
     Billing organization" is the team's size), mentorship, process-heavy
     change control, ownership.
3. **Fit** (see [below](#fit)).
4. **Contradictions** are first-class, not score penalties.
5. **Semantic review** (see [below](#semantic-review)).
6. **Practicality** is read from the gate and the practical signals.
7. **Today** ([`feed::qualifies_for_today`](../crates/jobhunt-app/src/feed.rs)):
   strong fits that aren't excluded, the person hasn't dealt with, best
   first, one per company (a company's other *strong* fits ride along);
   at most 5 (10 on request). Today may hold one job or none. Plausible
   fits ("worth reviewing") stay in search (`narrow find`, `search_jobs`)
   and never fill Today. Notifications take strong fits recommended
   outright, as before.

## Fit

The fit of a job for a person ([`FitAssessment`](../crates/jobhunt-ranking/src/fit.rs))
holds affirmative reasons, contradictions, uncertainties, the aspects
evaluated, and a level:

| Level | When | Tier |
| --- | --- | --- |
| strong | the work fits, at least one more aspect affirmatively fits, enough of it is the person's own, and nothing contradicts or holds it; with a firm view on the company side (company, team, ownership, culture), some of that fits too | strong fit (Today) |
| plausible | something points to it, not enough | worth reviewing |
| insufficient | little or nothing does | maybe |
| poor | a material contradiction | low priority |

A job never earns a recommendation because nothing rejected it: role fit
must be affirmative (the shape of work they want, or a specialty they want
or have shown), and a second aspect must be too. What counts as evidence:

| Aspect | For | Against (contradiction) |
| --- | --- | --- |
| role | the work shape they want: the title's (full weight), the description's, or work a product/full-stack role includes (¾), a related shape (½) | a shape they avoid; not engineering at all; a team's name standing in for unrelated work ("iOS Engineer - User Platform"; security work for a cloud team still counts) (holds) |
| depth | a specialty they want, or have demonstrated in their career evidence | a specialty they neither want nor have shown ("using PostgreSQL is not building its storage engine"); deep work when they said they want broad work |
| seniority | the level they want (a founding role counts as senior) | a level they avoid; two steps from what they want (early career for a senior); one step below a level they stated holds (one step below their latest title is only noted) |
| company | the stage or size they want | one they avoid; a large or public company when they want startups or small companies (and nothing larger) |
| team, culture, ownership | what they want | what they avoid; a large team when they want small ones; change-control processes when they want ownership (holds) |
| domain, technology, way of working | what they want (technology counts half: overlap isn't role fit) | what they avoid |
| this job | they liked or saved it | they disliked it |

Pay is not an aspect of fit. Nor are remote work, verification or
freshness.

### Whose statement it is

Fit reads the composed taste profile (`jobhunt_profile::taste::compose`):
the person's own statements, Narrow's reading of their words, structured
preferences set earlier (read live, so nobody redoes onboarding), and
learned patterns, merged by key with the person's decisions first.
Removed statements are tombstones and say nothing; neutral statements
("doesn't matter") give no signal either way. How much a statement counts
([`Firmness`](../crates/jobhunt-ranking/src/fit.rs)):

| Firmness | Statements | A match counts | A contradiction is |
| --- | --- | --- | --- |
| firm | written, confirmed or corrected by them; an earlier setting they entered | 1 | material |
| soft | Narrow's reading of their words, an inference from their profile (medium confidence or better) | 0.6 | material |
| weak | a low-confidence reading or inference | 0.3 | holding |
| learned | a pattern learned from feedback, never confirmed | 0.3 | holding when established with high confidence, else only said |

So learned-only taste never silently overrides what the person said
(compose keeps their statement for the same key), and never makes a strong
fit on its own. Without a stated level, the level of their latest title
stands in (soft): an early-career role is still poor for a senior.

### Reasons people read

Reasons answer "why would I actually want this?", with the posting's words:

- "Senior backend work, the kind of engineering you want"
- "A startup of 15 people, the kind of company you want: “Quarrybird is a 15-person startup…”"
- "A small team, as you want: “You'll join a team of 6 engineers and own backend services end to end…”"
- "Building database-engine internals is the core of the role, the work you want"
- "Uses PostgreSQL the way you have (using a database in applications), not database-engine internals"

Contradictions say why a relevant-looking job is held back ("An
early-career role, which you said you don't want (Early-career roles)",
"Deep specialist work in database-engine internals (OrioleDB, storage
engine, write-ahead logging), while you want broad work rather than
specialist roles"). They are in every ranking (`narrow why --details`,
the API's `decision.against`, the benchmark report), not all shown on
Today.

## Practicality

[`Practicality`](../crates/jobhunt-ranking/src/practicality.rs) is `clear`,
`check`, `concern` or `impossible`, with:

- **blockers**: eligibility rules it out, the stated work setup or
  relocation conflicts, the person asked to hide such jobs;
- **concerns**: pay below a floor or target;
- **things to check** (the person's own unresolved requirements first):
  "Unresolved: you require at least USD 120,000 per year, and this job's
  pay can't be checked against it", "Pay isn't published: unknown, not
  low", "Eligibility unclear: the listing says “Remote” but publishes no
  geographic scope", "Unresolved: the listing says remote, but it also
  expects office presence or travel", "Verify it's still open";
- **facts**: pay meeting the target, remote from where they want.

### Compensation

| Situation | Fit | Practicality |
| --- | --- | --- |
| not published | unchanged | a thing to check; a strong fit still makes Today |
| meets the target or floor | unchanged (never a reason) | a fact |
| below a *required* floor, verified | unchanged | a concern; excluded from recommendations (the person said required) |
| below a wanted floor or target | unchanged | a concern |
| a range labeled for another location ("US base salary range" for someone in Brazil) | unchanged | not compared at all: "The published pay range is for the United States; what it pays in Brazil isn't stated" |

High pay alone never puts a job on Today: the benchmark's "surfaced only
because of pay" is 0 by construction (pay signals weigh nothing).

## Semantic review

The rules decide on their own when no reviewer is configured (the default,
the open-source path, and what the benchmark measures). A reviewer reads
the shortlist more closely than the rules can: postings whose vocabulary
the rules don't know, work the title doesn't name.

- **Seam**: [`FitReviewer`](../crates/jobhunt-ranking/src/review.rs); the
  model implementation is [`jobhunt_ai::ModelFitReviewer`](../crates/jobhunt-ai/src/lib.rs)
  (the Anthropic Messages API with structured outputs, or any
  OpenAI-compatible server), sharing the taste reader's configuration,
  retries and logging.
- **Only the shortlist**: strong and plausible fits, not excluded, best
  first, at most 30 looked up and 12 new reviews per ranking, 4 at a time,
  no new call after 45 seconds (`ReviewBudget`). Everything else never
  reaches a model.
- **Structured output only**: a fixed JSON schema (fit, role/company/
  seniority/specialization verdicts, quoted affirmative points,
  quoted contradictions, uncertainties). `parse_review` rejects malformed
  answers whole and drops any point whose quote isn't verbatim in the
  posting (it can't invent company facts), any aspect outside the
  vocabulary, and anything about pay, visas, relocation or where the job
  may be done. A strong verdict without a quoted role fit and another
  quoted reason becomes plausible; a poor verdict without a quoted
  contradiction becomes insufficient.
- **The model never decides the ranking**: `review::apply` combines
  deterministically. A rules-poor job stays poor; a quoted contradiction
  makes a job poor; a model's doubt holds a strong fit back to plausible;
  a model can raise a plausible fit to strong only with quoted evidence and
  no holding contradiction from the rules; an insufficient one at most to
  plausible.
- **Failure is conservative**: unavailable, timeout, rate limit, refusal,
  truncation or a malformed answer (after the provider's retries) leave the
  rules' assessment standing, marked `Unavailable`, never stored, asked
  again next time. A model outage never fills Today with weaker jobs; it
  never empties it either.
- **Cached**: `fit_reviews` (SQLite; Postgres sealed like rankings, key
  rotation included), keyed by the reviewer (provider and model), the
  prompt and rules revisions, a digest of what was sent about the person,
  and the posting's text as sent. The same (person, posting version,
  reviewer) is never reviewed twice; a corrupt stored review is reviewed
  again. `explain` reads stored reviews and never calls.
- **Configuration**: off by default. Locally `[ai] review_fit = true`
  (optionally `review_model`); in the cloud `JOBHUNT_AI_REVIEW_FIT=true`
  (optionally `JOBHUNT_AI_REVIEW_MODEL`) with the `JOBHUNT_AI_*` provider
  settings. The cloud reviews in the background notification pass after
  discovery, so Today's requests mostly find reviews stored.

### Exactly what is sent

`FitReviewRequest::build` is the only place a request is assembled;
`review::render` turns it into the text after the fixed `SYSTEM_PROMPT`.

- **The candidate**: each taste statement as "polarity · dimension: value
  (phrase) · whose" ("prefer · team: small_team (Small technical teams) ·
  confirmed"); the level of their latest title ("senior (latest title)");
  specialties their career evidence shows ("database-engine internals");
  for up to 6 visible experiences the title, the years, and the topics of
  role and domain claims and technologies (up to 8).
- **The job**: title, company name, the location and workplace fields,
  Narrow's rule reading as hints (level, shapes, specialties, company,
  team), and the description's content sentences without benefits and pay
  boilerplate (at most 60 sentences, 7,000 characters).

Never sent: the person's name, email, phone, contacts, location, employers'
names, experience summaries, resume, LinkedIn or GitHub text, education,
feedback reasons, pay preferences, ids. Prompts and answers are never
logged; logs carry the provider, model, attempt, duration, fit level,
rejected points and token counts.

### Cost

On the production-shaped snapshot below, the shortlist (strong and
plausible fits, capped at 30) produced review prompts of 5,800–11,900
characters including the fixed instructions and schema: roughly 1,500–
3,000 input tokens. With an answer of 300–600 tokens (plus thinking at
`effort: low`), a review costs about $0.02 with Claude Opus 5.5 ($4 / $20
per million tokens), about half with Claude Sonnet 5.5 ($2 / $10,
`review_model`). Reviews are per (person, posting version, model): a
person's first rankings review their shortlist a dozen at a time (about 30
reviews, ~$0.60, once); after that only new or changed shortlisted
postings cost anything, typically a handful a day (~$0.10–0.25 per person
per day). Nothing outside the shortlist ever reaches a model. The live cost
wasn't measured here (no API key in this environment); the logs record
tokens per review (`fit reviewed`) and per ranking (`fit review`).

## Production-shaped validation

The 17 boards the cloud reads (`deploy/cloud.toml`: Linear, Ramp, Notion,
Supabase, PostHog, Modal, Replit, Vanta, Anthropic, Stripe, Figma, Airbnb,
Spotify, Palantir, Zoox, DoorDash, PostHog/YC) were discovered into a
throwaway local database on 2026-09-30: **2,894 open jobs**. The person was
the benchmark's synthetic senior startup generalist (Brazil, remote only,
backend/platform, small teams, ownership, startups or growth companies; no
early-career, deep-specialist, large or process-heavy), set up through the
CLI; nothing touched production state, and no private data was used. The
same snapshot was ranked by the previous ranker (version 5) and this one.

| | Before (v5) | After (v6) |
| --- | --- | --- |
| Open jobs | 2,894 | 2,894 |
| Ruled out deterministically (ineligible; stated work-setup, relocation or geography conflicts) | 2,725 | 2,716 |
| Candidates for Today | 64 "worth reviewing" | 10 strong fits (21 plausible, 28 insufficient, 119 poor) |
| On Today | 7 companies | 3 companies |

The version 5 Today: Supabase's "Supalite Engineer" with 36 roles riding
along (support engineers, developer relations, event programs, managers,
OrioleDB), an Airbnb senior role with an early-career peer, an Anthropic
**research** role carried by pay ("Reaches your target"), a Spotify
**Director of Engineering** for an individual contributor, a Stripe backend
role and a DoorDash distributed-databases role on unconfirmed eligibility.

The version 6 Today: Supabase's platform and infrastructure roles (Compute
Capacity, Edge & Networking, AI Platform, Release, Supalite, Edge
Functions, Platform Security: growth-stage, "systems you own", "own the
infrastructure end to end"), a Stripe backend role ("a high level of
autonomy"; eligibility and pay to check) and a PostHog product-engineering
role at a Y Combinator company ("Autonomy: we don't tell anyone what to
do"). Held back, with the contradiction said: people-management roles
(Engineering Manager, Director of Engineering), Supabase's Postgres and
OrioleDB internals work, early-career roles, a large company's full-stack
role, customer-facing solution architects and support engineers, mobile
and ML roles on teams merely named "Platform".

Reviewing the real results drove five fixes the synthetic fixtures didn't
exercise: people management is a contradiction for someone whose career and
taste are individual-contributor work; developer-relations, community,
program and product-lead titles aren't engineering; ownership needs the
posting to say the person owns something ("autonomous agents" and
"end-to-end coding agents" are product copy); a team's name ("… - User
Platform") doesn't stand in for the title's own unrelated work; and a
generic title's work is never read from sentences about the company ("We
provide a complete backend solution").

### Performance

Release build, the 2,894-job snapshot, one person:

| | Cold (first ranking in a process) | Warm (postings already read) |
| --- | --- | --- |
| Load records and verification | 1.6 s | 1.2 s |
| Eligibility (cached decisions after the first) | 2.6 s | 0.4 s |
| Reading the work and assessing fit | 5.0 s | 0.13 s |
| Total | 9.3 s | 1.8 s |

Each posting is read once per version per process (`facets_of`) and
shared by every person's ranking. With a reviewer, the semantic stage adds
at most 12 calls (4 at a time, 45 s budget) per ranking until the
shortlist is reviewed, then cache lookups only.

## Feedback

Save, Not for me, Not now and I applied stay what they were: taste-learning
signals with their evidence, kept apart from what the person said
([`taste`](../crates/jobhunt-ranking/src/taste.rs), composed as learned
statements). "Not for me" offers one-click reasons (Wrong seniority, Too
specialized, Wrong kind of work, Company too big, Company stage, Domain,
Location, Compensation) that only add words to the free text; the reader
(`rules/2`) reads them against the job: "Too specialized" is the job's
specialty, "Wrong kind of work" its work shape, "Company stage" its stage,
"Wrong seniority" its level (the title's or the description's).

## Compatibility

- Ranking `RANKING_VERSION` 6: every stored ranking key changes; old rows
  stay as a record.
- No onboarding again: structured preferences compose live into the taste
  profile; stored taste statements, confirmations, corrections and learned
  patterns are read as they are.
- Additive storage: the `fit_reviews` table (SQLite and Postgres
  migrations `20261007000000_fit_reviews`); nothing else changes.
- API: `ShortlistItem` (and so `FeedItem`) gains `fit`, `to_check` and
  `facts`; `DecisionView` gains `fit`, `against` and `facts`;
  `FeedSummary` gains `strong_fits`. All additive.
