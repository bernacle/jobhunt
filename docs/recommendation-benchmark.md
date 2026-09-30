# The recommendation benchmark

A deterministic, offline benchmark of whether the jobs Narrow labels
*worth your attention* are ones a reasonable candidate would genuinely
consider applying to. It exists so that preference and ranking changes
(BRU-321, BRU-322) can be judged by what they do to Today, rather than by
which jobs happen to move. It measures; it does not change ranking.

- Code and fixtures: [`crates/jobhunt-eval`](../crates/jobhunt-eval)
- Recorded baseline: [`crates/jobhunt-eval/baseline/recommendation.md`](../crates/jobhunt-eval/baseline/recommendation.md)
- What ranking work must preserve: [ranking-invariants.md](ranking-invariants.md)

```bash
# The report (Markdown), for every candidate or one
cargo run -p jobhunt-eval --bin narrow-eval -- recommendation-benchmark
cargo run -p jobhunt-eval --bin narrow-eval -- recommendation-benchmark --candidate senior-startup-generalist --details

# The suite CI runs: fixture integrity, evaluator, baseline in sync
cargo test -p jobhunt-eval

# After an intended ranking change: regenerate the baseline, review its diff
JOBHUNT_UPDATE_BENCHMARK=1 cargo test -p jobhunt-eval --test recommendation_benchmark
```

## How Today is produced today

The audit the benchmark is built on (ranking version 5, eligibility rules
version 4). Everything below is deterministic: there is **no LLM anywhere
in the pipeline**. The resume parser (`DeterministicParser`), the
preference statement parser (`RuleParser`) and the feedback reason reader
(`RuleReader`) are rule-based; the `ResumeParser`, `StatementParser` and
`ReasonReader` traits are seams for AI-assisted readers that don't exist.

1. **Discovery** (`jobhunt-sources`, `jobhunt-jobs`): Ashby, Greenhouse,
   Lever and YC boards into canonical `JobPosting`s, deduplicated into
   opportunities; lifecycle events record material changes.
2. **Normalization**: `jobhunt_ranking::facets` reads each posting into
   facts with their words: role shapes (title words, or the description
   for generic titles), a level from title words (`LEVEL_TERMS`),
   technologies weighed by section (required / preferred / mentioned),
   domains, company and team kinds stated in the description, work style
   keywords, work modes. `jobhunt_eligibility::job` reads the posting's
   requirements (remote scopes, offices, authorization, time zones).
3. **Eligibility** (`jobhunt-eligibility`): the person's facts (location,
   authorization, work modes, relocation, time zones) against those
   requirements: eligible, conditional, uncertain or ineligible, with
   reasons.
4. **Verification** (`jobhunt-jobs::verification`): whether the listing is
   live at an authoritative source, recently enough to trust. Today
   verifies up to twice its limit of candidates before choosing.
5. **Preferences** (`jobhunt_ranking::person`): stated roles, company and
   team kinds, domains, work styles, work modes, remote geographies and pay
   (`Person`), kept apart from resume experience (role kinds, domains,
   technologies with evidence strength, the level of the latest title).
6. **Taste** (`jobhunt_ranking::taste`): patterns learned from feedback
   with their evidence; stated preferences always win. The benchmark runs
   with no feedback (empty learned taste).
7. **Signals** (`jobhunt_ranking::signals`): one weighted, inspectable
   signal per finding, summed into a score. Roughly: a stated role or domain
   +2, other wanted things +1.5, remote as preferred +1, pay meeting a
   minimum +1 and a target +1.5, stack used +0.5 each (at most 1.5),
   experience +0.5, eligible +0.5, verified +0.5, fresh +0.25; a refused
   role −3, "not one of the roles you listed" −1, a level two or more below
   the latest title −1.5, one below −0.5, **above it 0** ("a stretch").
   Unknowns weigh nothing.
8. **Gate** (`rank::gate`): excluded (rejected, in the pipeline, closed,
   ineligible, a stated work-setup or relocation conflict, verified pay
   below a required minimum, a required company/team kind or remote
   geography the posting contradicts, unknown pay or unclear eligibility
   when the person hides them); else eligibility unclear; else verify
   first; else recommended.
9. **Tier** (`rank::tier`): strong fit at score ≥ 4 with a positive signal
   of a personal basis (stated, learned, feedback); worth reviewing at ≥
   1.5; maybe at ≥ −1; else low priority. A signal of weight ≤ −2 caps it
   at maybe; an unresolved requirement (a required pay floor, team or
   company kind, or remote geography the posting doesn't settle, or a work
   setup it leaves unclear) caps it at worth reviewing.
10. **Brief** (`jobhunt_ranking::brief`): the verdict, "why it may be worth
    your time" (plus signals, personal ones first), "things to consider"
    (minus, conditions, blockers) and unknowns: the material concerns.
11. **Today** (`jobhunt_app::feed`): among the rankings that are not
    excluded, those at least **worth reviewing**, whatever the gate
    (recommended, verify first **or eligibility unclear**), that the
    person hasn't dealt with (or that changed materially since), in rank
    order, **one per company**, at most 5 (10 on request). The company's
    other qualifying jobs ride along as `also_at_company` ("also at this
    company", the peers). Nothing pads it: fewer qualifying jobs is a
    shorter Today, none is "caught up". Notifications take only strong
    fits that are recommended outright.

So a job becomes:

| Outcome | How |
| --- | --- |
| worth your attention (Today) | not excluded, tier ≥ worth reviewing, new or materially changed, best of its company within the limit |
| also at this company (peer) | qualifies, but another job at the same company ranks higher |
| excluded | a gate exclusion (see 8) |
| unresolved | an unknown signal ("Unresolved: …"); tier capped at worth reviewing; or the eligibility-unclear gate. Still eligible for Today |
| impossible | eligibility says ineligible (or a stated conflict): excluded |

## Benchmark design

### Fixtures

```text
crates/jobhunt-eval/fixtures/recommendation/
  jobs/golden.toml          the four production failures, paraphrased from public postings
  jobs/positive.toml        jobs Narrow should surface
  jobs/contrastive.toml     pairs: postgres, kubernetes, ai, seniority, company-shape
  jobs/compensation.toml    below the floor, high pay on weak fit, a range for another location
  jobs/practicality.toml    on-site, other regions, US-only (read and unread)
  candidates/<id>/resume.md       the candidate's resume
  candidates/<id>/candidate.toml  preferences, what the model can't express, judgments
```

Jobs are a compact form of the canonical `JobPosting` (compensation is the
canonical `Compensation`). A candidate's profile is built the way
`narrow init` builds one: the Markdown resume through the production
`DeterministicParser` and `merge_resume`, then preferences as canonical
`PreferenceValue`s with the stances the app sets. Ranking is the
production `jobhunt_ranking::rank` on the production eligibility
assessment, with every listing verified active six hours before a fixed
clock (as Today verifies its candidates). All people and most companies
are made up; the four golden companies' postings are paraphrased, not
copied. Nothing reads the network, a database or a model.

A candidate is judged only on the jobs in their pool (the jobs they have
a judgment for). Fit belongs to the candidate, never to the job.

### Candidates

| Candidate | Who | Why it's there |
| --- | --- | --- |
| `senior-startup-generalist` | ~10 years backend/platform/product infra (TypeScript, Node, Go, AWS, Kubernetes as a user, PostgreSQL as an app database); Brazil, remote only, no relocation; wants small teams, high ownership, startups | Stands in for the person the production failures were observed on. 27 judgments |
| `database-internals-specialist` | Storage-engine engineer (WAL, MVCC, C/C++/Rust, PostgreSQL contributor); Germany, EU-authorized | OrioleDB and StrataDB are Strong yes here |
| `early-career-generalist` | One year out of university, TypeScript backend; Brazil | Airbnb Early Career and the Orbitly new-grad role are Strong yes here; senior roles are No |
| `us-platform-engineer` | Kubernetes operators and control plane, upstream contributor; Denver, US-authorized | Stripe and US/Canada-only roles are practical here; the control-plane role is a Strong yes |

### Judgments

Every (candidate, job) judgment answers two questions separately:

- **Fit**: `strong_yes` (exactly what they'd seriously consider), `maybe`
  (plausible, something meaningful missing), `no` (meaningfully wrong).
- **Practicality**: `valid`, `unknown` (not stated: pay, remote scope),
  `concern` (a known downside that doesn't rule it out: pay below the
  floor), `impossible` (a concrete condition rules it out).

They make the label: **Impossible** when practicality is impossible, else
the fit (**Strong yes**, **Maybe**, **No**). The label says what Today
should do:

| Label | Today |
| --- | --- |
| Strong yes, practicality valid or unknown | surface it (not surfacing it is a **miss**) |
| Strong yes with a practical concern | either |
| Maybe | hold it back (surfacing it is a **maybe in Today**) |
| No, Impossible | never (surfacing it is a **false positive**) |

Each judgment carries structured reasons from a small taxonomy
(`jobhunt_eval::Reason`), checked for consistency when the fixtures load:
a strong yes needs affirmative fit evidence and no contradiction or thin
evidence (pay is never fit evidence); a no needs a contradiction; a maybe
says what is missing; impossible names the condition; valid practicality
carries only valid practical reasons.

- Fit, for: `seniority_match`, `engineering_work_match`,
  `specialization_match`, `company_shape_match`, `team_shape_match`,
  `ownership_match`, `domain_match`, `technical_depth_match`,
  `startup_match`
- Fit, against: `seniority_mismatch`, `engineering_work_mismatch`,
  `specialization_too_deep`, `company_shape_mismatch`,
  `team_shape_mismatch`, `ownership_mismatch`, `domain_mismatch`,
  `technical_depth_mismatch`, `large_company_mismatch`
- Fit, thin: `weak_positive_evidence`, `insufficient_fit_evidence`
- Practicality: `geography_valid`, `compensation_good`,
  `compensation_below_target` (valid); `geography_unclear`,
  `compensation_unknown`, `compensation_range_not_applicable`,
  `travel_or_office_unclear` (unknown); `compensation_below_floor`
  (concern); `geography_invalid`, `work_authorization_invalid`,
  `remote_scope_invalid`, `relocation_invalid`, `timezone_invalid`
  (invalid)

Contradiction categories are derived from the reasons: **seniority**
(`seniority_mismatch`), **role depth** (`specialization_too_deep`,
`technical_depth_mismatch`, `engineering_work_mismatch`), **eligibility**
(the invalid practical reasons) and **company shape**
(`company_shape_mismatch`, `large_company_mismatch`, `team_shape_mismatch`).

### Metrics

No aggregate score; each is reported on its own, overall and per
candidate. "Surfaced" means labeled worth your attention: not excluded
and at least worth reviewing, which is what Today selects from. The
simulated feed (best of each company, at most five) is reported too, but
its size depends on the pool, so precision is measured on what surfaced.

- **Strict Today precision**: surfaced jobs judged Strong yes.
- **Relaxed Today precision**: surfaced jobs judged Strong yes or Maybe.
- **Actionable precision**: surfaced jobs that fit enough (Strong yes or
  Maybe) and have no known practical problem (valid or unknown).
- **Obvious false positives**: surfaced jobs judged No or Impossible.
- **Practical Strong yes surfaced**: the guard against a precise but empty
  Today.
- **Contradiction miss rate** per category: judged contradictions that
  surfaced anyway, and how many the ranker has no negative signal for.
- **Density**: surfaced jobs, and jobs on the simulated feed.
- **Surfaced only because of pay**: ranked again without the candidate's
  pay preferences, the job no longer qualifies.
- **Pay ranges for another location read as meeting the candidate's pay**.
- **Surfaced with eligibility unconfirmed** (descriptive).

### CI and determinism

`cargo test --workspace` (CI's `test` job) runs the crate's tests:

- **Benchmark integrity must pass**: the fixtures parse and are consistent
  (every judgment's reasons agree with its verdicts, every job is judged,
  every pair has both sides, golden jobs name the candidate they were
  observed on, each candidate has something to find and something to
  avoid), the golden, contrastive, archetype and compensation expectations
  the benchmark promises are encoded, the evaluator classifies outcomes
  correctly, and the report is deterministic.
- **Current quality is recorded, not asserted**: cases failing their
  judgment don't fail CI. The report of the current ranker is committed as
  `baseline/recommendation.md`, and `baseline_is_current` fails when the
  ranker's behavior on the benchmark changes and the baseline wasn't
  regenerated. A ranking change therefore arrives with its before/after
  diff; BRU-322 can then turn important invariants into hard gates
  (`narrow-eval recommendation-benchmark --strict` exits non-zero on any
  failing case).

The benchmark never calls a model. Future AI-assisted ranking should be
evaluated on the same fixtures with recorded model outputs (fixtures of
the model's answers, or a pinned model snapshot), never a live call in CI.

## Baseline (ranking version 5)

49 judgments, 4 candidates, 27 jobs. The full per-case report is the
[baseline file](../crates/jobhunt-eval/baseline/recommendation.md).

| Metric | All candidates | Senior startup generalist |
| --- | --- | --- |
| Strict Today precision | 18/37 (49%) | 11/21 (52%) |
| Relaxed Today precision | 21/37 (57%) | 12/21 (57%) |
| Obvious false positives | 16/37 (43%) | 9/21 (43%) |
| Maybes in Today | 3 | 1 |
| Practical Strong yes surfaced | 18/18 (100%) | 11/11 (100%) |
| Surfaced only because of pay | 4 | 3 |
| Foreign pay range read as meeting pay | 1/1 | 1/1 |
| Surfaced with eligibility unconfirmed | 5 | 3 |
| Seniority contradictions surfaced | 6/8 (75%) | |
| Role-depth contradictions surfaced | 9/13 (69%) | |
| Eligibility contradictions surfaced | 2/11 (18%) | |
| Company-shape contradictions surfaced | 3/4 (75%) | |

The current ranker finds every practical Strong yes; its problem is
precision. Almost half of what it labels worth your attention is a job the
candidate would dismiss or can't take.

### The four golden cases (senior startup generalist)

| Case | Expected | Current | Today? | Result | Why it surfaced |
| --- | --- | --- | --- | --- | --- |
| Airbnb, Software Engineer, Early Career | No (seniority) | recommended, worth reviewing (score 5.75) | yes | false positive | "Early Career" is not a level Narrow reads, so there is no seniority signal at all; a wanted backend role (+2), Latin America remote (+1), Java/Python used, eligible and verified carry it. Capped at worth reviewing only because pay is unknown against the required minimum |
| Stripe, Backend Engineer, Payments Infrastructure | Impossible (US only) | eligibility unclear, **strong fit** (7.25) | yes | false positive | "Remote in United States" and "open to US-based candidates" are not read as a scope, so eligibility is unclear rather than ineligible; Today admits eligibility-unclear jobs at full tier, and pay (+2.5) and backend (+2) make it a strong fit. The control posting with "US-Remote" is correctly excluded |
| Supabase, OrioleDB Developer | No (role depth) | recommended, worth reviewing (5.25) | yes | false positive | The description makes it a backend role; the stated backend preference (+2), PostgreSQL experience and worldwide remote carry it. Nothing distinguishes building a storage engine from using PostgreSQL |
| Anthropic, Staff+ Software Engineer, Privacy | Maybe (thin fit) | eligibility unclear, worth reviewing (2.75) | yes | maybe in Today | Pay (+2.5) is what lifts it over the worth-reviewing threshold (ranked without pay preferences it doesn't qualify); "not one of the roles you listed" (−1) is outweighed. Large-organization Staff+ leadership is invisible to the model |

For the other candidates the same postings behave as expected where the
model can tell: Airbnb Early Career is a strong fit for the early-career
candidate, and excluded (Brazil-only remote) for the others; OrioleDB
surfaces for the database specialist, but only as worth reviewing, because
their stated "storage engine" role doesn't match the title "OrioleDB
Developer" (−1). OrioleDB is also a false positive for the early-career
candidate, and Stripe for them too (unreadable US scope).

### Positive cases the current ranker misses

None: every practical Strong yes surfaces (18/18). Some rank lower than
they should: Lanternfish (pay unpublished) is capped at worth reviewing by
the unresolved pay floor, below false positives like StrataDB (strong fit,
9.25) or Consolidated Meridian (strong fit, 7.50); the specialist's
OrioleDB and the US platform engineer's Helmsman control-plane role (their
Strong yeses) are only worth reviewing, because their stated roles don't
match the titles. With a full production pool, those can lose their feed
slot to generic matches.

## Gaps the benchmark exposes (for BRU-321 and BRU-322)

In the preference model (BRU-321):

- **No seniority preference.** Level is only compared with the latest
  resume title. A senior can't say "not early-career"; a junior can't say
  "not senior". Recorded per candidate under `unexpressed` in the fixtures.
- **No specialization or engineering shape.** Nothing expresses "product
  and platform generalist, not database internals / model training /
  low-latency trading", or the opposite ("storage engines, deeply").
  The role vocabulary has no database-internals role; a stated "storage
  engine" role only matches titles containing those words.
- **Ownership and company shape are keywords.** "Ownership" matches the
  word; company and team size are known only when the posting states them
  in words Narrow reads, and "startup" wanted is never contradicted by a
  posting that says "publicly traded" or "Fortune 500".

In ranking (BRU-322):

- **Additive generic positives reach worth reviewing.** Eligibility,
  verification, freshness, remote-as-preferred and experience signals sum
  past the 1.5 threshold without any affirmative fit evidence (Northstar's
  research role reached 3.75 on pay, geography and eligibility alone).
- **Pay creates fit.** Pay meeting a minimum (+1) and a target (+1.5) is
  added to the same score, and counts as a personal basis for a strong
  fit; 4 surfaced jobs qualify only because of pay.
- **Location-labeled ranges are read as the person's pay.** A range
  labeled "US base salary range" is compared with the person's target.
- **Seniority is weak and one-sided.** At most −1.5 for two or more levels
  below, 0 for above ("a stretch"), never a cap. "New Grad" is detected
  and still surfaces for the senior; "Early Career" isn't read at all;
  senior roles surface for the junior with no signal.
- **Stack match ignores depth.** "Asks for PostgreSQL, which you've used"
  is the same signal for an app database and for its storage engine;
  description-inferred roles ("backend" for a storage-engine or
  control-plane posting) satisfy stated role preferences.
- **Eligibility-unclear jobs enter Today at full tier.** Five surfaced
  jobs rest on unconfirmed eligibility, including a strong fit whose US-only
  restriction was simply unread.
- **Detected contradictions don't gate.** The ranker has a negative signal
  for 19 of the 36 judged contradictions (eligibility 9 of 11, role depth
  9 of 13, seniority 1 of 8, company shape 0 of 4). Apart from eligibility,
  which excludes, a signal only subtracts from a sum: the new-grad role
  (−1.5 seniority) and the research and trading roles ("not one of the
  roles you listed", −1) still surface.

## Separate correctness findings (not fixed here)

Found while building the fixtures; each is an input-reading issue
independent of ranking semantics, reported rather than fixed in this
benchmark change:

1. **Eligibility doesn't read some US-only phrasings as a scope**:
   location "Remote in United States" (the way Stripe's jobs site lists
   remote locations) and "Remote in North America", and the description
   sentence "This role is open to US-based candidates." Each yields
   "Remote with no geographic scope" (uncertain) instead of a US scope.
   "US-Remote", "Remote, US", "US-Remote, CA-Remote" and "must be based in
   the US" are read correctly.
2. **Team size is skipped in clauses naming the organization**: "You'll
   join a team of 60 engineers in the Billing Services organization" gives
   no team size, because `team_size` ignores any clause containing a
   company word ("organization"), even when it is about the team joined.
3. **"Early Career" is not a level term** (`LEVEL_TERMS` has "new grad",
   "graduate", "junior", "entry level"). A vocabulary gap in ranking's
   reading of titles; left for BRU-322 since adding it changes rankings.
