# Experiment: real-posting precision with wider sources and semantic review (BRU-325)

Run date: **2026-10-01**. A measurement; it changes no ranking, threshold,
eligibility rule, reviewer budget, source list or deployment. The judged
set is in
[`crates/jobhunt-eval/fixtures/real-postings/bru325-2026-10-01.json`](../crates/jobhunt-eval/fixtures/real-postings/bru325-2026-10-01.json).

## 1. Executive summary

All four cells were measured on one frozen corpus: the same 4,622
postings, candidate, roles, binary, instant and feed path. The reviewer
cells used the existing `ModelFitReviewer` with OpenAI `gpt-6-luna` and the
production budget, unchanged.

| Steady state | A1 · current · off | A2 · current · on | B1 · wider · off | B2 · wider · on |
| --- | ---: | ---: | ---: | ---: |
| Strong fits | 13 | **0** | 33 | 5 |
| Strict precision, strong tier | 3/13 (23%) | — | 8/33 (24%) | 1/5 (20%) |
| Obvious false positives, strong tier | 2 (15%) | 0 | 11 (33%) | 3 (60%) |
| Practical Strong yes, strong tier (jobs · companies) | 3 · 2 | 0 · 0 | 8 · **5** | 1 · 1 |
| Today (companies · jobs) | 4 · 13 | **empty** | 5 · 14 | 3 · 5 |
| Strict precision, Today jobs | 3/13 (23%) | — | 3/14 (21%) | 1/5 (20%) |

**Wider sources improve coverage.** Practical Strong-yes companies go from
2 to 5 with the reviewer off. Today doesn't show it yet: misread jobs take
the five company slots.

**The semantic reviewer fails the success bar.**

- *What it gets right:* it removes most of the known false positives
  (Curriculum Developer, GTM Engineer, ZK, block storage, bare metal,
  Multigres in one of two runs), with sound quotes.
- *What it gets wrong:*
  - It demotes almost every genuine Strong yes. 1 of 8 practical Strong
    yeses survives in B, 0 of 3 in A. It reviews like a résumé-gap
    screener: anything the candidate's evidence doesn't explicitly show
    ("1+ year in a DevOps or SRE role", "curiosity of OS level
    primitives", "storage internals" once in a description) means *not
    strong*.
  - Its verdicts agree with the judge on 7/23 (A2) and 12/30 (B2) reviews.
  - Identical requests got a different verdict 4 times out of 12.
- *Today, first load:* the 12-review budget means the unreviewed tail
  replaces what was removed. B's first Today is led by **Axiom's *ZK
  Proof Engineer*** and **Stripe's US-only role**, neither of them
  reviewed.
- *Today, steady state:* A's Today is **empty**. B's has 3 companies, 1
  of them good; the rest are US-only Temporal roles (geography is
  practicality, not the reviewer's job) and 3 jobs the shortlist can
  never reach.
- *Cost and latency:* about $0.05 for both cells. The first Today load
  takes ~70 s instead of ~10 s.

**Decision: B: wider sources work, the reviewer fails.**

- *PR #37:* keep, unchanged; it is already merged.
- *Next:* a narrower semantic-reading experiment (§22), plus the separate
  eligibility patch. Not more hand-written rules.

## 2. Experimental design

| Cell | Sources | Reviewer | Database |
| --- | --- | --- | --- |
| A1 | 17 current production boards | off | `/tmp/bru325/A1` |
| A2 | 17 current production boards | on (`gpt-6-luna`) | `/tmp/bru325/A2` |
| B1 | the 17 + the 43 BRU-323 remote-first boards | off | `/tmp/bru325/B1` |
| B2 | the 17 + the 43 BRU-323 remote-first boards | on (`gpt-6-luna`) | `/tmp/bru325/B2` |
| A0, B0 (added) | as A1, B1 | off | the same, roles removed (for the PR #37 decision) |

**What is identical across cells:**

- the frozen posting versions (§3);
- the candidate: same taste digest, same roles (§4);
- the binary: `main` at `8942300`, ranking v6, PR #37 merged;
- the instant: `now` = 2026-10-01T20:05:00Z;
- the feed call.

**What differs:**

- A2 and B2 are fresh copies of the frozen corpus with an empty review
  cache. Parity with A1 and B1 was checked: job ids, content fingerprints
  and the taste hash all match.
- Their config differs from A1 and B1 only by an `[ai]` block (`provider`,
  `model`, `review_fit = true`). The API key came from the process
  environment, was never written to a config, and was deleted after the
  runs.

**How each cell was ranked.**

- `crates/jobhunt-cli/examples/real_posting_probe.rs` calls the production
  `LocalApp::feed` with `limit` 5 (the web default), `refresh: never`,
  `verify: false`.
- Today is printed exactly as the feed builds it: strong fits only, one
  company per slot, the company's other strong fits riding along, at most
  5 companies, and the existing handled/skipped state (none).
- With a reviewer, the probe repeats the feed until nothing is deferred.
  This is the steady state of a person opening Today a few times. It also
  records every call.

**First load.**

- The live first pass is recorded per pass: counts and the Today list.
- Its full rankings were re-derived without calls
  (`/tmp/bru325/firstload.sh`): the production feed runs on a copy that
  keeps only the 12 first-pass reviews, with the reviewer pointed at a
  closed port, so every other shortlisted job keeps the rules' verdict, as
  a deferred job does.
- The replayed first Today matched the live one in both cells.

**Verification.** `verify: false` keeps the snapshot frozen. The probe was
rerun with `verify` on copies of A1 and B1: 10 candidates were verified
per cell, and Today was identical.

## 3. Frozen corpus

- **Fetch once.** One discovery-only run (`narrow find --raw --refresh`)
  of all 60 sources into one SQLite database: 2026-10-01
  20:04:14–20:04:50 UTC, 0 failures, **4,622 open postings**. It was not
  refetched for the reviewer cells.
- **Freeze.**
  - Check-pointed and copied read-only to
    `/tmp/bru325/frozen/corpus-2026-10-01.db`, sha256
    `251076834bb7dd2d4af235070ee14014b28013f0f2dad4eea53bffdfe118d224`
    (re-verified before A2/B2).
  - Every cell is a copy. A's copies delete the 1,726 postings (and their
    events and evidence) of the 43 added boards: **2,896 open**.
- **What's committed.** The 50 judged postings, as derived data only:
  - ids, URL, company, title, location fields, date and age;
  - the content fingerprint;
  - the per-cell tier;
  - the rules' gate, practicality, reasons and contradictions (≤200
    characters);
  - the model's verdicts with their quotes (≤120 characters) and token
    counts;
  - the judgment with one short quote.

  No description text is committed.
- **Boards.** Every BRU-323 board exists. `greenhouse:remotecom` answers
  but lists **0 jobs** (62 on 2026-09-30). It was kept as is, not
  replaced.

## 4. Candidate

The BRU-323/324 synthetic senior startup generalist, from BRU-324's
"after" database (job data removed, profile untouched):

- **Practical constraints:** São Paulo, remote only, no relocation,
  authorized in Brazil, no pay floor.
- **Roles:** Backend · Platform · Infrastructure (PR #37's picker).
- **Taste:**
  - prefer: senior, IC, small team, high ownership, broad, startup,
    growth, strong engineering;
  - avoid: early career, management, research, database internals, large
    company, process-heavy.
- **Digest:** `CandidateBrief::digest` `924957be043c…` in A1, A2, B1 and
  B2; `873b07bfc876…` without roles (A0, B0).

## 5. Model and reviewer configuration

| Setting | Value |
| --- | --- |
| Provider | OpenAI, through the existing OpenAI-compatible adapter (`response_format` JSON schema, `strict: true`) |
| Model | `gpt-6-luna`; reviewer name `model/openai:gpt-6-luna` |
| Why this model | The cheapest the existing adapter can use unchanged. Anthropic's Haiku 4.5 rejects the `effort` field the Anthropic adapter always sends. `gpt-6-luna` passed a structured-output smoke test (strict schema, no refusal, usage reported) before the runs. |
| Effort / reasoning | not sent (the OpenAI path sends none); `completion_tokens` include the model's reasoning tokens |
| Reviewer / prompt version | `fit-review/1`, with `fit-rules/…` in the key |
| Budget (production, unchanged) | shortlist 30 strong + plausible, best first · 12 new reviews per ranking · concurrency 4 · 45 s deadline for starting calls |
| Per-call timeout · retries | 40 s · 1 |
| Validation | strict schema; quotes verbatim in the posting; points about pay, visas, relocation, remote or time zones dropped; strong needs a quoted role point plus another; poor needs a quoted contradiction |
| Authority | the combination table in `review.rs`: it may hold back or demote; it may raise plausible → strong only on quoted evidence and without a holding rules contradiction; it never touches practicality |
| Price | $0.10 / $0.50 per million input / output tokens ([OpenAI pricing](https://developers.openai.com/api/docs/pricing), 2026-10-01) |

**Fit-only.** The reviewer receives the location fields (`JobBrief.setup`)
but is told not to use them. Points mentioning them are dropped: 4 in B2,
0 in A2. Fit and practicality stay separate. **Geography errors are not
the reviewer's to fix**, and they stay visible in §14.

## 6. The 2 × 2

Definitions:

- **Strict:** Strong yes.
- **Relaxed:** Strong yes or Maybe.
- **Obvious false positive:** No or Impossible.
- **Practical Strong yes:** Strong yes with practicality Valid or Unknown
  (BRU-320).

Reviewer cells are at steady state. First load is shown in §8.2.

| Metric | A1 | A2 | B1 | B2 |
| --- | ---: | ---: | ---: | ---: |
| Open jobs | 2,896 | 2,896 | 4,622 | 4,622 |
| Ruled out by practicality (remote-only/no-relocation conflict · ineligible) | 2,721 (2,239 · 482) | same | 4,135 (2,648 · 1,487) | same |
| Actionable | 175 | 175 | 487 | 487 |
| Insufficient | 22 | 22 | 54 | 54 |
| Plausible | 26 | 21 | 77 | 90 |
| Poor | 114 | 132 | 323 | 338 |
| **Strong** | 13 | **0** | 33 | 5 |
| **Today jobs · companies** | 13 · 4 | **0 · 0** | 14 · 5 | 5 · 3 |
| Strong tier: strict | 3/13 (23%) | — | 8/33 (24%) | 1/5 (20%) |
| relaxed | 11/13 (85%) | — | 22/33 (67%) | 2/5 (40%) |
| obvious FP | 2/13 (15%) | 0 | 11/33 (33%) | 3/5 (60%) |
| impossible | 1 | 0 | 6 | 3 |
| practical Strong yes (jobs · companies) | 3 · 2 | 0 · 0 | 8 · 5 | 1 · 1 |
| Today jobs: strict · obvious FP | 3/13 (23%) · 2 | — · 0 | 3/14 (21%) · 3 | 1/5 (20%) · 3 |
| Today leads: strict · obvious FP | 2/4 · 1 | — | 1/5 · 2 | 1/3 · 1 |
| Practical Strong-yes companies on Today | 2 | 0 | 2 | 1 |
| Top-10 plausible: strict · obvious FP | 1/10 · 5 | 3/8 · 3 (2 unjudged) | 1/10 · 6 | 4/10 · 3 |

Contradictions among strong fits judged not Strong yes. A job may count
under more than one heading.

| Failure | A1 | A2 | B1 | B2 |
| --- | ---: | ---: | ---: | ---: |
| Role shape | 1 | 0 | 2 | 0 |
| Specialization depth | 7 | 0 | 13 | 2 |
| Seniority | 0 | 0 | 2 | 1 |
| Management / non-engineering / customer-facing | 0 | 0 | 4 | 0 |
| Company shape | 3 | 0 | 5 | 1 |
| Eligibility (Impossible) | 1 | 0 | 6 | 3 |

B2's remaining false positives are Temporal's US-only roles (eligibility)
and two jobs beyond the shortlist the reviewer never saw. Demoted Strong
yeses land in the plausible tier, which is why B2's top-10 plausible holds
more Strong yeses (4) than B1's (1).

Without the explicit role (A0, B0: same corpus, reviewer off): **0 strong
fits** and an empty Today in both universes. B0's plausible list still
opens with ClickHouse *Curriculum Developer* and Oyster *GTM Engineer*.

## 7. Source coverage

**Hypothesis:** a better-aimed universe produces materially more good
companies.

- **With the reviewer off: yes.**
  - Practical Strong-yes companies in the strong tier go from 2 to 5
    (Supabase, PostHog; plus Railway, Oyster, Socket).
  - Practical Strong-yes jobs go from 3 to 8.
  - That meets the ~2× target.
- **On Today: no.**
  - Today keeps 2 practical Strong-yes companies (Supabase, and Oyster
    only as a peer under its GTM lead). PostHog drops out of the
    5-company cap.
  - Misread jobs at new companies rank above good ones: their boilerplate
    supplies strong startup/team/ownership support, and VerifyFirst gates
    rank above EligibilityUnclear ones.
- **With the reviewer on:** coverage collapses in both universes (2 → 0;
  5 → 1), because the reviewer demotes good jobs too. The added sources
  still contribute the only practical Strong yes left (Railway).
- **Where the gain came from** (B1):

| Added boards (42 non-empty + Remote.com) | |
| --- | --- |
| Postings | 1,726 |
| Actionable | 312 (Canonical alone: 187) |
| Boards with any actionable job | 22 of 43 |
| Boards with a strong fit | 8 |
| Practical Strong yes | Railway 3, Oyster 1, Socket 1 |

- **"Remote-first" is not "hires in Brazil".** 21 added boards yield
  nothing actionable:
  - Grafana, Attio, Tailscale, Vercel, Sentry, Render, Warp, LaunchDarkly
    and others hire remotely only in named US or European countries.
    These exclusions are correct.
  - Wildlife Studios, EBANX and Pipedrive are office-based; the São Paulo
    Senior Backend role is excluded by the person's remote-only
    constraint, correctly.
  - Wikimedia is the exception: wrongly excluded (§14).
- **Noise.**
  - Canonical (1,200+ people, a company shape the persona avoids) supplies
    60% of the new actionable jobs.
  - One small board, Railway (8 postings), supplies 3 of the 5 new
    practical Strong yeses.

**Verdict.** The wider universe roughly doubles the good companies the
ranker can find. The property that matters for this audience is "names
Brazil/LATAM/worldwide in its remote scope", not "remote-first". Its value
reaches Today only if reading precision improves.

## 8. Reviewer results

### 8.1 What the model did

| | A2 | B2 |
| --- | ---: | ---: |
| Reviews performed (passes 1 · 2 · 3) | 30 (12 · 12 · 6) | 30 (12 · 12 · 6) |
| Verdicts: strong · plausible · poor | 0 · 12 · 18 | 2 · 13 · 15 |
| Plausible jobs raised to strong | 0 | 0 (no plausible job is shortlisted in B) |
| Failures · retries · malformed · refused | 0 · 0 · 0 · 0 | 0 · 0 · 0 · 0 |
| Points dropped in validation (unquoted or practical) | 0 | 4 |
| Strong fits deferred on the first load | 1 of 13 | 21 of 33 |
| Strong fits that can never enter the 30-job shortlist | 0 | 3 (Temporal *SWE II*, Temporal *Staff, Traffic*, Anthropic *Inference Velocity*) |
| Cache hits on the warm rerun | 30/30, 0 calls | 30/30, 0 calls |

**Agreement with the judge**, mapping Strong yes → strong, Maybe →
plausible, No → poor:

| Judge → model | A2 | B2 |
| --- | ---: | ---: |
| Strong yes → strong · plausible · poor | 0 · 3 · 1 | 2 · 6 · 1 |
| Maybe → strong · plausible · poor | 0 · 3 · 10 | 0 · 5 · 9 |
| No → strong · plausible · poor | 0 · 2 · 4 | 0 · 2 · 5 |
| **Agreement** | **7/23 (30%)** | **12/30 (40%)** |

- **It never calls a No strong.** It removed 9 of 13 reviewed No's.
- **It rarely calls anything strong.** Strong-yes jobs get *plausible*
  because:
  - level, company stage or team size are "not established";
  - Supabase's company-wide "~400 team members" counts against a small
    team;
  - the candidate's evidence doesn't explicitly show a listed
    requirement.

  Strong needs "nothing that goes against", so an unstated fact is enough
  to deny it.
- **Instability.** 12 requests were identical in A2 and B2 (same review
  key, separate cold calls). **4 got different verdicts**:
  - Supalite: poor vs plausible;
  - Multigres Deployment: poor vs plausible;
  - FinOps: poor vs plausible;
  - Stripe Core Technology: plausible vs poor.

  Caching makes a person's view stable once reviewed, but which verdict
  gets cached is a coin flip on a third of the cases.

### 8.2 First load vs steady state

| | A1 (off) | A2 first load | A2 steady | B1 (off) | B2 first load | B2 steady |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Strong | 13 | 1 | 0 | 33 | 21 | 5 |
| Today companies · jobs | 4 · 13 | 1 · 1 | 0 · 0 | 5 · 14 | 5 · 13 | 3 · 5 |
| Strict, Today jobs | 3/13 | 0/1 | — | 3/14 | 4/13 (31%) | 1/5 |
| Obvious FP, Today jobs | 2 | 0 | 0 | 3 | 4 | 3 |
| Obvious FP, Today leads | 1/4 | 0/1 | — | 2/5 | 2/5 | 1/3 |
| Strong tier: strict · obvious FP | 3/13 · 2 | 0/1 · 0 | — | 8/33 · 11 | 5/21 · 8 | 1/5 · 3 |
| Practical Strong-yes companies on Today | 2 | 0 | 0 | 2 | 2 | 1 |

- **First load.**
  - The 12 best-ranked strong fits are reviewed, and every one of them is
    demoted in both cells.
  - The unreviewed tail moves up. In B the first Today has 5 companies and
    13 jobs, **none of them reviewed**: Canonical (2022 and 2021
    postings), Railway (7 jobs: 3 Strong yes, 2 Maybe, 2 No), Socket,
    **Axiom *ZK Proof Engineer*** (a No, now a lead) and **Stripe's
    US-only role**.
  - Strict precision rises to 4/13 only because Railway's unreviewed
    Strong yeses fill the slots.
- **Steady state** (3 loads, about 2.5 minutes of model time in total).
  - A: nothing.
  - B: Railway *Senior Product Engineer, Scalability* (the one reviewed
    and confirmed Strong yes); Temporal (a US-only lead the model rightly
    calls strong on fit, plus 2 unreviewable peers); Anthropic *Inference
    Velocity* (unreviewable).

### 8.3 Known problems: did the reviewer fix them?

| Known problem | Result |
| --- | --- |
| ClickHouse *Curriculum Developer & Instructor* | **Fixed**: poor. "Developing ClickHouse training courses"; "Teaching ClickHouse courses both virtually and in person" |
| Oyster *GTM Engineer* | **Fixed** on fit: poor ("Sitting in Revenue Operations, you'll work alongside GTM Teams"). The EMEA-only scope is still an eligibility bug. |
| Supabase *Multigres Deployment* (K8s internals) | **Inconsistent.** A2: poor ("experience building production-grade operators or controllers", "Deep Kubernetes expertise"). B2, the identical request: plausible (held back). Off Today both times. |
| ZK / cryptography (Axiom *ZK Proof*) | **Fixed at steady state**: poor ("implement and optimize cutting-edge cryptographic code for zero-knowledge proof generation"). **Not on the first load**: unreviewed, it becomes a Today *lead*. |
| Storage / kernel / bare metal | **Fixed**: Railway *Storage* poor ("log structured block-storage system"), *Baremetal* poor ("PXE boot, Ansible, and burn-in agents"), *Datacenters* poor. **Overreach:** Railway *Infrastructure Engineer* (a Strong yes) poor over "curiosity of OS level primitives" and "Build system-level software" |
| Customer-facing architects | **Partly**: Temporal *Platform Architect* held at plausible ("Lead cross-functional coordination of Professional Services…"), not removed. Supabase *Customer Solution Architect* and Stripe *Integration Engineer* stay plausible. |
| Manager / director | **Not exercised.** The rules already rank 161 of 163 such titles poor, so none is shortlisted. |
| "Platform" only as a team or product name | **Mixed**: Sourcegraph *Platform [IC3]* held (plausible), Temporal *Platform Architect* held, Supabase *Platform Security* poor. But real platform roles are demoted too: Oyster *Senior Engineer (Platform)* and Socket *Senior Platform Engineer* go to plausible. |

### 8.4 The upper bound, revisited

Before the model ran, the judge's labels were run through the same
combination table and budget. That bounds what a reviewer that always
agreed with the judge could do: B's steady Today at 5 companies and 7/10
strict, and 8/8 with the eligibility patch of §14. The real reviewer
reaches 1/5. The architecture can carry a good reviewer; this reviewer,
with this prompt and this model, isn't one.

## 9. Exact Today lists

**A1** (reviewer off). The same 4 companies and 13 jobs as BRU-324's
"after".

| | Job | Judged |
| --- | --- | --- |
| lead | Supabase: Platform Engineer - Compute Capacity | **Strong yes** |
| + | Supabase: Release Engineer | Maybe |
| + | Supabase: Supalite Engineer | **Strong yes** |
| + | Supabase: Platform Engineer, Edge & Networking | Maybe |
| + | Supabase: Edge Functions Engineer | Maybe |
| + | Supabase: Platform Security Engineer (AMER/APAC) | Maybe |
| + | Supabase: FinOps Engineer | Maybe |
| + | Supabase: Multigres Deployment Engineer | **No** (K8s internals) |
| lead | Stripe: Backend Engineer, Core Technology | **Impossible** (`US-Remote, …`; 838 days old) |
| lead | Anthropic: Senior+ Software Engineer, Legal Tech | Maybe (large company; Concern) |
| + | Anthropic: Staff+ Software Engineer, Inference Velocity | Maybe (Concern) |
| lead | PostHog: Product Engineer | **Strong yes** (Unknown) |
| + | PostHog: ClickHouse Operations Engineer | Maybe |

**A2** (reviewer on).

| Load | | Job | Reviewed? | Judged |
| --- | --- | --- | --- | --- |
| first | lead | Anthropic: Staff+ Software Engineer, Inference Velocity | no (deferred) | Maybe (Concern) |
| steady | — | *empty: "Nothing worth your time yet"* | | |

**B1** (reviewer off).

| | Job | Judged |
| --- | --- | --- |
| lead | ClickHouse: Senior Curriculum Developer & Instructor | **No** (not engineering) |
| lead | Oyster: Senior GTM Engineer | **Impossible** (EMEA only; and No: revenue ops) |
| + | Oyster: Senior Engineer (Platform) | **Strong yes** (Unknown) |
| lead | Supabase: Platform Engineer - Compute Capacity | **Strong yes** |
| + | Supabase: the same 7 peers as A1 | 1 Strong yes, 5 Maybe, 1 No |
| lead | Sourcegraph: Software Engineer - Platform [IC3] | Maybe (below senior) |
| lead | Canonical: Software Developer (Backend SaaS) | Maybe (large; posted 2022-10-20) |
| + | Canonical: Senior Software Engineer - MAAS | Maybe (posted 2021-03-29) |

**B2** (reviewer on), first load. None of these 13 jobs was reviewed.

| | Job | Judged |
| --- | --- | --- |
| lead | Canonical: Software Developer (Backend SaaS) | Maybe |
| + | Canonical: Senior Software Engineer - MAAS | Maybe |
| lead | Railway: Senior Infra Engineer: Baremetal Orchestration | Maybe |
| + | Railway: Senior Infra Engineer: Observability | Maybe |
| + | Railway: Senior Platform Engineer: Storage | **No** |
| + | Railway: Senior Full-Stack Engineer - Product | **Strong yes** |
| + | Railway: Infra Engineer - Datacenters | **No** |
| + | Railway: Infrastructure Engineer | **Strong yes** |
| + | Railway: Senior Product Engineer, Scalability | **Strong yes** |
| lead | Socket: Senior Platform Engineer | **Strong yes** (Unknown) |
| lead | Axiom: ZK Proof Engineer | **No** |
| + | Axiom: Distributed Systems Engineer | Maybe (Concern) |
| lead | Stripe: Backend Engineer, Core Technology | **Impossible** |

**B2**, steady state.

| | Job | Reviewed? | Judged |
| --- | --- | --- | --- |
| lead | Railway: Senior Product Engineer, Scalability | yes: strong | **Strong yes** |
| lead | Temporal: Senior Software Engineer, Cloud Platform Foundations | yes: strong | Strong yes on fit, **Impossible** (US cities only) |
| + | Temporal: Software Engineer II, Open Source Server | never (beyond the shortlist) | **Impossible** (and No) |
| + | Temporal: Staff Software Engineer, Traffic | never (beyond the shortlist) | **Impossible** |
| lead | Anthropic: Staff+ Software Engineer, Inference Velocity | never (beyond the shortlist) | Maybe (Concern) |

**A0, B0:** empty.

## 10. Manual judgments

- **Scope:** every strong fit and the top 10 plausible jobs of A1 and B1,
  50 distinct postings, judged before the reviewer cells ran. They were
  **not changed** after seeing the model's answers; a script asserts the
  judgments are byte-identical. Two plausible jobs that surfaced only in
  A2 (DoorDash *Core Platform*, Spotify *Enterprise AI*) are reported as
  unjudged.
- **Judge:** one judge (Claude, for the synthetic persona, not the real
  person), reading title, location fields and description.
- **Consistency with earlier labels:** they match BRU-323/324 except
  Railway *Storage*, re-judged **No** from Maybe ("filesystems, block
  devices, kernel I/O paths, SPDK, io_uring") before any review existed.

Treat the figures as ±10–15 points (one judge, small samples). The
direction is not in doubt.

## 11. False-positive taxonomy (rules)

Of B1's 33 strong fits, 25 are not strict yeses.

| Cause | Jobs (B1 strong tier) | Mechanism | Reviewer (B2) |
| --- | --- | --- | --- |
| Role inferred from requirements, not function | ClickHouse *Curriculum Developer*, Oyster *GTM Engineer* | title and department don't override requirement keywords | removed both |
| Company, team or ownership from boilerplate | "startup" from an award name and a founder bio; "ownership" from "customer love" and a heading | phrase match anywhere | n/a (fit verdicts don't repair the reasons) |
| Same-company boilerplate lifts every posting | 8 Supabase and 7 Railway strong fits | §18 | demoted Strong yeses and Maybes alike |
| Specialization depth unread | Multigres, Railway *Storage* and *Baremetal*, Axiom ZK, Edge & Networking, Edge Functions, Platform Security, ClickHouse ops, Anthropic toolchains | depth rules don't cover these | removed most; Multigres inconsistent |
| Seniority | Temporal *SWE II*, Sourcegraph *[IC3]* | "II"/"IC3" not read | IC3 held; SWE II never reviewed |
| Customer-facing | Temporal *Platform Architect* | the title reads as platform | held at plausible |
| Large company | Anthropic ×2, Canonical ×2 | size unread unless stated | Canonical poor; Anthropic Legal Tech held |
| Eligibility | Stripe, Temporal ×4, Oyster GTM | §14 | out of scope (fit-only) |

Below Today, in B1:

- **Manager, director and head-of titles:** 161 of 163 are poor.
- **Developer Relations:** 3 of 3 held at insufficient.
- **Field engineers:** 7 of 9 at insufficient or poor; 2 plausible.
- **Solutions, customer success, support and consulting titles:** 14 of 47
  still plausible.

## 12. Reviewer examples: successes and misses

Each example gives the rules' classification, the semantic review result
and quoted evidence, the final classification, and the judgment.

| Kind | Job | Rules | Review (verdict · quoted evidence) | Final | Judged |
| --- | --- | --- | --- | --- | --- |
| **Success: true positive retained** | Railway: Senior Product Engineer, Scalability | strong | **strong**: "This is a backend-leaning role focused on scaling systems."; "your remit spans every high-throughput system at Railway" | strong, Today lead | Strong yes |
| **Success: non-engineering role removed** | ClickHouse: Senior Curriculum Developer & Instructor | strong, B1 Today lead | **poor**: role: "Developing ClickHouse training courses"; work style: "Teaching ClickHouse courses both virtually and in person" | poor | No |
| **Success: non-engineering role removed** | Oyster: Senior GTM Engineer | strong, B1 Today lead | **poor**: role: "Sitting in Revenue Operations, you'll work alongside GTM Teams."; specialization: "Deep, hands-on knowledge of the modern GTM stack." | poor (still EMEA-only) | No / Impossible |
| **Success: specialization removed** | Railway: Senior Platform Engineer: Storage | strong | **poor**: "our new log structured block-storage system" | poor | No |
| **Success: Maybe held back** | Sourcegraph: Software Engineer - Platform [IC3] | strong, B1 Today lead | **plausible** | plausible | Maybe |
| **Miss: Strong yes demoted to poor** | Railway: Infrastructure Engineer | strong | **poor**: "Build system-level software…"; "Have a strong understanding or curiosity of OS level primitives" | poor | Strong yes |
| **Miss: Strong yes demoted to poor** | Supabase: Supalite Engineer (A2) | strong | **poor**: "comfortable moving across the stack from API design to storage internals to infrastructure"; company: "~400 team members". In B2 the same request got *plausible*. | poor / plausible | Strong yes |
| **Miss: Strong yes held back** | Socket: Senior Platform Engineer | strong | **plausible**, no contradiction. Uncertainties: "1+ year in a DevOps or SRE role" and observability/CI/CD "not shown in the candidate's career evidence" | plausible | Strong yes |
| **Miss: Strong yes held back** | PostHog: Product Engineer | strong | **plausible**: "focused on building an awesome product for end users"; "does not specify a seniority level" | plausible | Strong yes |
| **Miss: inconsistent** | Supabase: Multigres Deployment Engineer | strong | A2 **poor** ("Deep Kubernetes expertise"); B2 **plausible** (no contradiction), identical request | poor / plausible | No |
| **Miss: Maybe pushed to poor** | Supabase: Platform Engineer, Edge & Networking | strong | **poor** (networking specialty) | poor | Maybe |
| **Structural miss: never reviewed** | Temporal: Software Engineer II, Open Source Server | strong | not shortlisted (31st+ strong fit) | strong, on Today | No / Impossible |
| **Structural miss: first load** | Axiom: ZK Proof Engineer | strong | not yet reviewed on the first load (poor later) | Today **lead** on the first load | No |

The reviewer created no new obvious false positive: it raised nothing.
The first-load ZK and Stripe leads are an effect of the budget, not of a
model verdict.

## 13. Why the reviewer misses

1. **The prompt's bar.** "strong only when the work itself fits and
   something more does … with nothing that goes against what they want."
   The model reads unstated level, stage or team size, and any requirement
   the candidate's evidence doesn't list verbatim, as "something that
   goes against". It behaves like an applicant-screening checklist, not
   like a recruiter judging what the person would want.
2. **The combination table.** Strong + plausible → plausible. A reviewer
   that says "plausible" for everything removes every strong fit.
3. **Thin candidate evidence.** The brief carries titles, years, topics
   and ≤8 technologies per role. "Has the candidate run CI/CD?" is
   unanswerable, and the model counts unanswerable against.
4. **Budget.** 12 reviews per Today load, a 30-job shortlist: the first
   load shows the unreviewed tail, and a wide universe has strong fits the
   shortlist never reaches.
5. **Non-determinism.** A third of identical requests flip. One cached
   answer becomes the person's permanent verdict.

## 14. Eligibility errors (practicality; still not fixed)

The reviewer is fit-only and changes none of these. They stay visible in
every cell.

| Posting | Fields | Narrow's reading | Correct | On Today in |
| --- | --- | --- | --- | --- |
| Stripe: Backend Engineer, Core Technology | `US-Remote, Chicago, Seattle, San Francisco` | "says 'Remote' but publishes no geographic scope" (unclear) | US only: impossible | A1, B2 first load |
| Stripe: Integration Engineer (Metronome) | `Remote`, structured location `US` | unscoped remote | US only | — |
| Temporal: Senior SWE Cloud Platform Foundations; SWE II; Staff Traffic | remote; only Seattle/Austin/San Francisco | "remote but names only Seattle; doesn't say where remote work is allowed" | US only | **B2 steady (3 of 5 jobs)** |
| Temporal: Senior Platform Architect | remote; only London/Manchester | the same | UK only | — |
| Oyster: Senior GTM Engineer | Spain, Romania, Portugal, Slovakia, Poland; "this role requires you to be based within EMEA" | eligible, no check | EMEA only | B1 |
| Railway: Infra Engineer - Datacenters | `Remote (United States)`; "available anywhere in the world" | unclear, and says why | unclear: correct | — |
| Anthropic ×2 | "Remote-Friendly (Travel-Required) \| San Francisco, CA \| Seattle, WA \| New York City, NY" | unscoped remote, travel unresolved | likely US hubs | A1 |
| Axiom ×2 | `New York; Remote` | unscoped remote | unclear | B2 first load |
| **Wikimedia** (12 postings mention Brazil) | `Remote`; "Countries: Brazil, Canada, Colombia, …" | **INELIGIBLE**, from "The anticipated annual pay range … for applicants based within the United States is…" | open to Brazil (EOR) | hidden |
| **Zapier**: Staff Backend Engineer, Commerce | `NAMER; APAC; EMEA`; "Location: Americas - North, Central and South America, EMEA, APAC" | **INELIGIBLE**, "limits remote work to APAC or EMEA" | open to Brazil | hidden |

**Smallest deterministic patch**, recommended and not implemented:

1. `US-Remote`, `Remote - US`, `Remote (United States)` and a bare `US`
   location are a US scope.
2. A remote job listing only places in one country (or one set of
   countries) is scoped to them, unless the description widens it.
3. A pay range "for applicants based within the United States" is a pay
   scope, never a hiring scope.

(1) and (2) remove every Impossible in all four strong tiers except
Oyster's, including all three of B2's steady false positives. Next in
line: "requires you to be based within <region>" (Oyster) and region
words in the description (Zapier).

## 15. Staleness

| Strong fits older than | A1 | A2 | B1 | B2 first load | B2 steady |
| --- | ---: | ---: | ---: | ---: | ---: |
| 90 days | 4 | 0 | 16 | 14 | 2 |
| 180 days | 1 | 0 | 10 | 10 | 0 |
| 365 days | 1 | 0 | 9 | 9 | 0 |

- **The wider universe adds stale inventory:** Canonical 2021–2022, Axiom
  2022 and 2024, Railway's evergreen 2024–2025 postings.
- **B2's first load shows it on Today:** Canonical 2022 and 2021 postings
  as lead and peer.
- **Age alone isn't a rejection rule.** Two of Railway's year-old postings
  are Strong yes.
- **Today doesn't order by age.** Narrow already shows age as a concern
  ("Posted N days ago; it may be filled") but doesn't use it to rank.

## 16. Cost and latency (measured)

| | A2 | B2 |
| --- | ---: | ---: |
| Input · output tokens (output includes reasoning) | 56,902 · 41,486 | 58,680 · 42,138 |
| Per review, median (input · output) | 1,887 · 1,417 | 1,919 · 1,416 |
| **Cost** at $0.10 / $0.50 per MTok | **≈ $0.026** | **≈ $0.027** |
| Per review | ≈ $0.0009 | ≈ $0.0009 |
| Call latency (median · p90 · max) | 14.9 · 18.2 · 22.9 s | 14.4 · 18.1 · 21.4 s |
| Review stage per Today load (passes 1 · 2 · 3) | 58.9 · 52.8 · 29.9 s | 58.9 · 45.9 · 32.1 s |
| Whole Today load (passes 1 · 2 · 3) | 69.6 · 55.8 · 33.5 s | 76.4 · 50.7 · 37.9 s |
| Warm Today load (all cached) | 6.9 s; review stage 12 ms | 10.4 s; review stage 14 ms |
| Reviewer off, same corpus | 8.0–11.3 s | 13.6–17.2 s |

- **Total experiment:** ≈ $0.05 for 60 reviews (plus a 195-token smoke
  test).
- **Continuous use is cheap at this price.** Only new or changed
  shortlisted postings cost anything after the first day.
- **The first three Today loads are slow.** Each takes about a minute,
  because the review stage runs inline in the feed request and the 45 s
  deadline only stops new calls from *starting*.

## 17. Cache correctness

The key is `REVIEW_VERSION`, `FIT_RULES`, the reviewer name
(provider:model), the candidate brief's digest and the serialized job
brief, stored per `(profile_id, review_key)`.

- **Checked empirically.**
  - A1/B1 and A2/B2 (same candidate) give identical keys for every shared
    job.
  - With the roles removed (B0), the digest changes and 0 of 70 keys
    match.
  - The warm reruns hit all 30 cached reviews with 0 calls.
  - The first-load replays hit exactly the 12 kept reviews.
- **Inputs covered:** posting content and rule reading, taste, career
  evidence, provider and model, reviewer and rules versions.
- **Not in the key, not bugs today:** `effort` (not configurable),
  `base_url`, the prompt text (relies on bumping `REVIEW_VERSION`; one
  revision so far), and a server-side refusal fallback's model.
- **Real but not a key bug:** the non-determinism of §8.1 means the first
  cached answer becomes permanent. Whether to cache, re-ask, or ask twice
  is a design question, recorded here and not changed.

No cache correctness bug was found, so no test was added.

## 18. Company peer roles

- **Is each peer independently required to be strong? Yes.**
  (`qualifies_for_today`; `one_per_company` only groups.)
- **Does the reviewer run on each peer? Yes**, as on any shortlisted job.
  B2 shows the limits: peers beyond the shortlist (Temporal *SWE II*,
  *Traffic*) are never reviewed and ride along on Today under a reviewed
  lead.
- **Can a strong company lift mediocre peers? In effect, yes, through
  reading, not the feed.**
  - Company and team evidence is per-company boilerplate: "Over $1B raised
    (including our $500M Series F)" on every Supabase posting, "we're a
    startup" and "small team, with high ownership" on every Railway one.
  - So any role-shape match there becomes strong: 8 Supabase strong fits
    (2 Strong yes) and 7 Railway (3 Strong yes).
  - The reviewer doesn't fix this selectively. It demotes good and
    mediocre peers alike.

## 19. PR #37 recommendation

**Merge unchanged.** It is already merged as `8942300`. Keep it; don't
revert.

- **Without an explicit role, Today is empty in both universes.** A0 and
  B0 have 0 strong fits.
- **Every practical Strong yes found depended on it.**
- **It doesn't create the false positives.** They head the plausible list
  in the same order without it.
- **"Merge with a proven precision guard" doesn't apply.** The reviewer
  measured here is not a working guard; it removes the good jobs with the
  bad.

## 20. Decision

**B — wider sources work, the reviewer fails.**

- **Sources.** Practical Strong-yes companies 2 → 5 with the same reading
  (B1 vs A1). That meets the ~2× bar; the universe matters.
- **Reviewer, against the success bar:**

| Bar | A | B | Met? |
| --- | --- | --- | --- |
| Obvious false positives cut by about half | strong tier 2 → 0; Today 2 → 0 (Today empty) | strong tier 11 → 3 (33% → 60% of a smaller tier); Today 3 → 3 | count yes, rate **no** |
| Meaningful increase in strict Today precision | 23% → no Today | 21% → 20% | **no** |
| Genuine Strong yes preserved | 0 of 3 practical | 1 of 8 practical | **no** |
| No model-created new obvious FPs | none | none (no raises); the first load surfaces unreviewed FPs through the budget | yes |

- **Why not A.** Precision didn't improve, and good-company coverage
  collapsed.
- **Why not C.** C needs a working reviewer, and this one didn't work.
- **Why not D.** The sources half worked.
- **Scope of the conclusion.**
  - One model (`gpt-6-luna`, the cheapest tier) was tested. A stronger
    model might agree more often.
  - But the dominant failure is structural: the prompt's "nothing that
    goes against" bar, thin candidate evidence, the combination table and
    the budget. A stronger model at the same seams would still be judged
    by those seams.

**Product thesis.** "Only the few worth your time" needs a reading guard,
and the existing semantic reviewer, as designed, is not one. Narrow's
demonstrated strengths remain:

- the practicality funnel (89–94% of postings set aside before fit,
  nearly all correctly, with the Brazil-specific bugs of §14);
- explicit intent;
- a wider, better-aimed universe.

## 21. Real-posting quality gate

The fixture now carries the judged set, the rules' readings and the
model's verdicts. It is the seed of an out-of-sample regression corpus.

- **Its labels must not be read into production logic.** Tests should
  measure generalization, for example "no Strong yes lost; no No in
  Today".
- **Don't tune rules or prompts against these 50 postings.** The next
  judged snapshot should come from a different date.
- **Limitations:**
  - one judge, and not the real person;
  - one synthetic persona;
  - 50 postings;
  - one snapshot (2026-10-01);
  - a selection-biased wider universe (BRU-323);
  - one model, one cold run per cell. §8.1 shows a third of verdicts can
    flip, so treat reviewer counts as ±a few jobs.

## 22. Next action

**Proposed next issue, not created:** *Narrow semantic reading:
classify the work, not the candidate.* An offline experiment on this
fixture, no wiring:

- **Ask a model one narrower question per posting:** what is the job's
  function, with quotes?
  - engineering IC, of which kind (backend, platform, infrastructure, …);
  - or non-engineering, customer-facing, management;
  - or deep-specialist (which specialty).
- **Leave the rest to the candidate's stated taste and the rules:**
  - Don't ask the model to judge the candidate's résumé against the
    requirements; that is where this reviewer fails.
  - Don't let "not stated" count against a job.
- **Success bar on the 50 judged jobs:**
  - every No removed;
  - every Strong yes kept;
  - agreement ≥ 80%;
  - stable across two runs.
- **Only if it passes,** decide how to wire it: budget and first-load
  behavior.

**Independent of it:** the three-rule eligibility patch of §14. It is
deterministic and small, and it removes all of B2's remaining steady-state
false positives.

## Reproduce

```bash
# build (main 8942300 or later with the probe)
cargo build --release -p jobhunt-cli --example real_posting_probe
N=target/release/narrow; P=target/release/examples/real_posting_probe

# candidate: BRU-324's "after" database with its jobs removed (profile kept)
#   delete from discovery_runs, eligibility_decisions, job_events, job_evidence,
#   job_verifications, jobs, opportunity_rankings, source_scans
# configs: [discovery]/[verification] of deploy/cloud.toml; A = its 17 sources;
#   B = those plus the 43 boards of competitive-recommendation-teardown.md
#   Appendix A. Reviewer cells add (key from the environment, never the file):
#     [ai]
#     provider = "openai"
#     model = "gpt-6-luna"
#     review_fit = true

# fetch once (discovery only), then freeze
$N --config B.toml --database frozen.db find --raw --refresh -v -n 1
# A's copy: delete jobs (and their job_events, job_evidence) of the 43 added sources

# rank each cell at the snapshot instant (arg 4: max feed passes; arg 5: "verify")
$P A1/config.toml A1/jobhunt.db 2026-10-01T20:05:00Z > A1/cell.json
OPENAI_API_KEY=… $P A2/config.toml A2/jobhunt.db 2026-10-01T20:05:00Z 12 > A2/cell-cold.json 2> A2/cell-cold.err
# warm rerun: the same command (0 calls); first load: /tmp/bru325/firstload.sh A2
```

The frozen database, cell outputs (cold, warm and first-load), judging
excerpts and analysis scripts (`analyze.py`, `analyze2.py`) are in
`/tmp/bru325/` on the machine that ran this. The sha256 above identifies
the corpus.
