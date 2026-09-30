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

## How Today is produced

Since BRU-322 (ranking version 6, eligibility rules version 5) Today is
built fit first, practicality second; the whole architecture is in
[fit-and-practicality.md](fit-and-practicality.md). In short:

1. **Discovery** and **eligibility** as before; eligibility now reads
   "Remote in United States" and "open to US-based candidates" as a US
   scope.
2. **Reading the work** (`facets::work`): level (title, then description),
   shape (the title's own role words first), specialized depth (storage
   engines, Kubernetes internals, model training, low-latency trading,
   privacy), company stage and size, the team someone joins, culture.
3. **Fit** (`jobhunt_ranking::fit`) against the composed taste profile
   (BRU-321), each statement weighed by whose it is: affirmative reasons,
   first-class contradictions (material, holding, minor), a level
   (strong, plausible, insufficient, poor). The tier is the fit level:
   strong fit, worth reviewing, maybe, low priority.
4. **Practicality** (`jobhunt_ranking::practicality`): blockers, concerns,
   things to check, facts. It never raises fit; pay weighs nothing.
5. **Semantic review** of the shortlist by a model, when configured (never
   in the benchmark).
6. **Today** (`jobhunt_app::feed`): strong fits only, not excluded, new or
   materially changed, one per company (its other strong fits ride along),
   at most 5. No quota: fewer strong fits is a shorter Today, none is
   "caught up". Notifications take strong fits recommended outright.

| Outcome | How |
| --- | --- |
| worth your attention (Today) | a strong fit, not excluded, new or materially changed, best of its company within the limit |
| also at this company (peer) | a strong fit, but another job at the same company ranks higher |
| worth reviewing (search only) | a plausible fit: something points to it, not enough |
| excluded | a gate exclusion (ineligible, a stated conflict, a verified required pay floor, …) |
| impossible | eligibility says ineligible (or a stated conflict): excluded |

### Before BRU-322 (ranking version 5)

The audit the benchmark was built on: signals summed into a score (a
stated role +2, pay meeting a minimum +1 and a target +1.5, stack used up
to +1.5, eligible and verified +0.5 each, …); a tier from the score
(strong fit at 4 with a personal basis, worth reviewing at 1.5); a gate on
eligibility and verification; Today took every job at least worth
reviewing, whatever the gate. Seniority, role depth and company shape were
at most small subtractions, "Early Career" wasn't a level, and a US-only
scope written "Remote in United States" was read as unscoped.

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
  candidates/<id>/candidate.toml  preferences, what the model can't express, the
                                  taste profile ([taste], BRU-321), judgments
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
- **Current quality is recorded, and the gate asserted**: the report is
  recorded as below, and since BRU-322
  `the_ranker_meets_the_recommendation_gate` fails on any case failing its
  judgment. The report of the current ranker is committed as
  `baseline/recommendation.md`, and `baseline_is_current` fails when the
  ranker's behavior on the benchmark changes and the baseline wasn't
  regenerated. A ranking change therefore arrives with its before/after
  diff (`narrow-eval recommendation-benchmark --strict` also exits
  non-zero on any failing case).

The benchmark never calls a model: it measures the rules, which decide on
their own without a reviewer. The semantic reviewer is tested with
recorded answers and scripted reviewers (`jobhunt-ai/tests/fit_review.rs`,
`jobhunt-storage` `semantic_review`), never a live call in CI.

## Results: before (ranking version 5) and after (ranking version 6)

49 judgments, 4 candidates, 27 jobs, unchanged by BRU-322 (no judgment or
fixture was edited). The full per-case report is the
[baseline file](../crates/jobhunt-eval/baseline/recommendation.md); the
BRU-320 version is in the history of that file.

| Metric | Before (v5) | After (v6) |
| --- | --- | --- |
| Strict Today precision (Strong yes) | 18/37 (49%) | 18/18 (100%) |
| Relaxed Today precision (Strong yes + Maybe) | 21/37 (57%) | 18/18 (100%) |
| Actionable precision | 21/37 (57%) | 18/18 (100%) |
| Obvious false positives (No or Impossible) | 16/37 (43%) | 0/18 (0%) |
| Maybes in Today | 3 | 0 |
| Practical Strong yes surfaced | 18/18 (100%) | 18/18 (100%) |
| Surfaced only because of pay | 4 | 0 |
| Foreign pay range read as meeting pay | 1/1 | 0/1 |
| Surfaced with eligibility unconfirmed | 5 | 1 (Parcelwise: a Strong yes whose remote scope isn't stated) |
| Seniority contradictions surfaced | 6/8 (75%) | 0/8 (0%) |
| Role-depth contradictions surfaced | 9/13 (69%) | 0/13 (0%) |
| Eligibility contradictions surfaced | 2/11 (18%) | 0/11 (0%) |
| Company-shape contradictions surfaced | 3/4 (75%) | 0/4 (0%) |
| Density: labeled worth your attention | 37 of 49 | 18 of 49 |
| Density per candidate (specialist · early career · senior · US platform) | 4 · 6 · 21 · 6 | 2 · 2 · 11 · 3 |
| Feed: on it / strict precision | 19 / 58% | 12 / 100% |
| Cases failing their judgment | 19 | 0 |

The contradictions the ranker still has no negative signal for are all
kept out of Today by something else: Stripe's and Anthropic's size (their
postings don't state it: unknown stays unknown; Stripe is impossible and
Anthropic's privacy specialization is a role-depth contradiction), OrioleDB's
level for the early-career candidate (not stated; its depth is), and
Copperleaf for the database specialist (no contradiction, but nothing fits:
insufficient).

`the_ranker_meets_the_recommendation_gate` (in `cargo test -p
jobhunt-eval`) now fails CI on any failing case, any obvious false
positive, any Maybe in Today, pay carrying a job, a foreign range read as
met, a contradiction surfaced, or a practical Strong yes lost.

### The four golden cases (senior startup generalist)

| Case | Expected | Before | After |
| --- | --- | --- | --- |
| Airbnb, Software Engineer, Early Career | No (seniority) | worth reviewing, in Today | **poor**: "An early-career role, which you said you don't want"; not in Today |
| Stripe, Backend Engineer, Payments Infrastructure | Impossible (US only) | eligibility unclear, strong fit, in Today | **excluded, ineligible**: "The listing limits remote work to the United States; you live in Brazil"; fit plausible (backend at the right level, nothing shown of the company, team or ownership they want) |
| Supabase, OrioleDB Developer | No (role depth) | worth reviewing, in Today | **poor**: "Deep specialist work in database-engine internals (OrioleDB, storage engine, write-ahead logging), while you want broad work rather than specialist roles" |
| Anthropic, Staff+ Software Engineer, Privacy | Maybe (thin fit) | worth reviewing on pay, in Today | **poor**: privacy engineering is a deep specialization they neither want nor have shown; pay is a practical fact only |

For the database-internals specialist, the same OrioleDB posting is a
**strong fit** ("Building database-engine internals is the core of the
role, the work you want"; "The deep specialization you want, and your
experience shows it"), on Today with its unpublished pay to check.
Airbnb Early Career is a strong fit for the early-career candidate
(early-career backend work, a dedicated mentor).

### Positive cases

All 18 practical Strong yeses surface, now as strong fits rather than
"worth reviewing": the small-startup backend and platform roles, the
international remote ones, Lanternfish with unpublished pay (strong; pay
to check), Parcelwise with an unscoped "Remote" (strong; eligibility to
check), Atlasfield with a US-only pay range (strong; "The published pay
range is for the United States; what it pays in Brazil isn't stated"), the
specialist's OrioleDB and StrataDB, the US platform engineer's Kubernetes
control-plane role, and the early-career roles for the early-career
candidate. Kestrel (a strong fit paying below a required floor) is
excluded by the floor, as the person's constraint says, with the fit
intact.

## Gaps BRU-320 exposed, and where they stand

- Seniority preference, specialization, engineering shape, company shape
  as taste: the taste profile (BRU-321), read by fit (BRU-322).
- Additive generic positives reaching Today: gone; fit needs affirmative
  role fit and a second aspect, and Today takes strong fits only.
- Pay creating fit: gone; pay is practicality and weighs nothing.
- Location-labeled ranges read as the person's pay: not compared.
- Seniority weak and one-sided: both directions, material.
- Stack match ignoring depth: specialties are read and need their own
  evidence; description-inferred shapes don't stand in for them.
- Eligibility-unclear jobs at full tier: they may be on Today only as
  strong fits, with the unclear scope to check.
- Detected contradictions not gating: material contradictions make the fit
  poor.

The three input-reading findings (below) are fixed.

## Correctness findings BRU-320 recorded, fixed in BRU-322

1. **US-only phrasings**: "Remote in United States", "Remote in North
   America", "Remote within Canada" are read as scopes; "open to US-based
   candidates" and "US-based applicants" limit where people can be
   (`jobhunt_eligibility`, `RULES_VERSION` 5; `rule_matrix` cases).
2. **Team size in clauses naming the organization**: "You'll join a team of
   60 engineers in the Billing Services organization" gives a large team;
   a company word before the team still makes the size the company's.
3. **"Early Career"** (and "New Graduate") are levels.
