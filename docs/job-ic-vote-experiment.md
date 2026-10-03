# Experiment: a binary, three-vote "software engineering IC?" classifier

Run date: **2026-10-03**. The last semantic-reading experiment. Its
predecessor is
[`job-function-only-fresh-snapshot-experiment.md`](job-function-only-fresh-snapshot-experiment.md).
It is an offline measurement: it changes no ranking, threshold, eligibility
rule, source, reviewer or deployment.

- Annotations:
  [`fresh-2026-10-03-job-ic.json`](../crates/jobhunt-eval/fixtures/real-postings/fresh-2026-10-03-job-ic.json).
- Ensembles (Phase 1, regression, Phase 2):
  [`fresh-2026-10-03-job-ic-runs.json`](../crates/jobhunt-eval/fixtures/real-postings/fresh-2026-10-03-job-ic-runs.json).
- Classifier: [`jobhunt_eval::job_ic`](../crates/jobhunt-eval/src/job_ic.rs).
- Probe: the `ic-*` commands of
  [`job_function_probe`](../crates/jobhunt-cli/examples/job_function_probe.rs).
- Offline re-score: `cargo test -p jobhunt-eval --test job_ic_runs`.

## Summary

| | Result |
| --- | --- |
| Phase 1, fresh blind sample (94) | **PASS** on every pre-registered gate |
| Wrong-function jobs kept out of IC | 44/44 in all 3 ensembles |
| Ordinary engineering kept as IC | 36/36 in all 3 ensembles |
| Non-IC postings voted IC | 0 |
| Voted IC-or-not decision identical across 3 ensembles | **93/94 (98.9%)**, bar 98% |
| Regression (the 145 earlier postings) | PASS: 94/95 and 50/50 stable, no ordinary engineering lost |
| Phase 2: strong tier | **0 of 9** judged Strong yes lost; Curriculum, Datacenters and FinOps held out |
| Phase 2: Today | the Curriculum lead leaves; obvious false positives 3 → 2; strict precision **2/12 → 2/12**; practical Strong-yes companies **1 → 1** |
| Cost | ≈ $0.0006 per posting version (3 votes); $1.09 for the whole experiment |

**Decision: the classifier works, but it doesn't materially improve
Today.**

- It is accurate, stable and safe, and it removes what it targets.
- After PR #43, almost nothing it targets reaches Today: eligibility
  already removed Oyster *GTM* and Temporal *Platform Architect*.
- What is wrong with Today now is fit, not function.

Recommendation (§10): stop using semantic classifiers to fix Today. Work
on fit and company evidence next. Keep `job-ic/1` available as a cheap
hygiene gate; don't integrate it now.

## 1. Hypothesis

> If the question is only "software engineering IC or not", with mixed
> postings forced to `unclear` and three independent votes per posting
> version, the answer is accurate on fresh postings and stable enough to
> cache (≥ 98%). As a Today gate, it removes Curriculum, GTM and
> customer roles without losing PostHog *Product*, Oyster *Platform*,
> Railway, Socket or Supabase's good jobs.

## 2. Pre-registration and timeline

| When (UTC) | What |
| --- | --- |
| 2026-10-03 00:06:27 | Classifier, prompt, schema, vote rule and gates committed (`d72f0d5`) |
| 00:11:53–00:12:11 | Snapshot fetched: 60 boards, 0 failures, 4,643 open postings (sha256 `b5cea521…`) |
| then | Sample drawn (seed `job-ic/1 2026-10-03`), excluding the 145 postings judged in BRU-325/330 and the 2026-10-02 experiment |
| 00:14:39 | Blind annotations committed (`3b29451`, sha256 `50d18e06…`), before any model call |
| 00:15:29 | First ensemble |

The prompt names job families, never titles from any fixture, and it was
frozen before any of the evaluated postings were read.

**Schema.** Quotes first, then one answer. Nothing else: no function
taxonomy, depth, specialty, seniority, customer contact, fit or
candidate. A unit test enforces this.

```json
{"evidence": ["short verbatim quote"], "answer": "software_engineering_ic | not_software_engineering_ic | unclear"}
```

**Prompt.** It is in `job_ic::SYSTEM_PROMPT`; the digest is
`328b2097…`. In short:

- *IC*: design, build and ship software hands-on for the employer's own
  product, platform or infrastructure.
- *Not IC*: management, customer work, developer relations, training,
  GTM or revenue systems, product or program management, research, or
  non-software work.
- *unclear*: a job split between engineering and another function, or a
  posting that says too little.
- Specialization, seniority and difficulty don't matter.
- "Don't pick a side for a mixed job."

**Input:** the job-only input of the earlier experiments (title,
department and team, content sentences). No company name, location,
pay or candidate data is sent.

**Vote.** Three independent calls per posting version:

- any `unclear` vote, or a missing vote, makes the answer `unclear`;
- otherwise the majority wins.

**Gates**, scored on the voted answers of 3 independent ensembles:

- voted agreement ≥ 95% in every ensemble;
- no posting whose annotation rules out IC is voted IC;
- every wrong-function job is voted out of IC, and every ordinary
  engineering job is voted IC, in every ensemble;
- the voted IC-or-not decision is identical across the three ensembles
  for ≥ 98% of postings.

The stability gate is on IC-or-not because that is all a Today gate sees:
`not` and `unclear` both keep a job off Today.

**Phase 2** runs if Phase 1 passes. The gate keeps a job eligible for
Today only when it is voted `software_engineering_ic`. Nothing else
changes: no score, no tier, no other rule.

## 3. Sample and annotation

The sample was drawn with the stratified title sampler of the earlier
experiment, unchanged, with the 145 earlier postings excluded:

- 94 postings drawn;
- the "Platform in name" stratum had only 2 eligible.

The annotation was done by one annotator (Claude), from exactly the text
the model is sent, after the classifier was frozen:

- 39 IC, 49 not IC, 6 mixed (`unclear`);
- critical sets: 44 wrong-function, 36 ordinary-engineering.

The traps include:

- Temporal *SWE II, Cloud Enablement* (IC; "Enablement" is a team name);
- Anthropic *Research Engineer, RL Velocity* (IC: builds training
  infrastructure);
- Palantir and Modal *Forward Deployed Engineer* (not IC);
- Vanta *AI GTM Engineer* (not IC, or unclear);
- Palantir *Product Reliability Engineer* in a support org (IC, or
  unclear);
- two marketing-site design engineers (mixed).

The regression sets (the 95 postings of 2026-10-02 and BRU-330's 50)
were derived mechanically from their function annotations, after the
fact. They are reported separately and don't decide anything.

## 4. Phase 1 results (fresh blind sample)

| | Ensemble 1 | Ensemble 2 | Ensemble 3 |
| --- | ---: | ---: | ---: |
| Voted agreement (strict) | 92/94 (89) | 92/94 (88) | 93/94 (91) |
| Wrong-function out of IC | 44/44 | 44/44 | 44/44 |
| Ordinary engineering voted IC | 36/36 | 36/36 | 36/36 |
| Non-IC voted IC · IC lost | 0 · 0 | 0 · 0 | 0 · 0 |
| Mixed postings voted `unclear` | 4/6 | 2/6 | 4/6 |
| Failed votes | 0 | 0 | 0 |

- **Stability:**
  - IC-or-not decision: **93/94 (98.9%)**;
  - three-way answer: 89/94;
  - unanimous ensembles: 270 of 282.
- **The only flip across the IC line:** Palantir *Web Design Engineer*.
  It was annotated mixed. Its ensembles voted unclear · IC · not; the IC
  ensemble had 2 IC votes and 1 not, with no `unclear`.
- **The other disagreements are all on the safe side:** non-IC postings
  voted `unclear` instead of `not`:
  - Tailscale *Manager, DevRel Engineering*;
  - ClickHouse *Consulting Engineer*;
  - Palantir *FDE*.
- **Quotes:** 2,195 valid, 31 invalid (dropped). No answer was left
  unsupported, and none mentions an applicant.

**PASS.**

## 5. Regression sets and a rate-limit incident

The first regression scores failed badly: up to 8 ordinary engineering
jobs lost in one ensemble, including PostHog *Product Engineer* and
Railway *Scalability*. **The cause was HTTP 429 rate limits, not the
model.**

- About 10 minutes into continuous calls, 41 of 1,305 votes failed after
  their one retry.
- A missing vote counts as `unclear`, so each failure hid an IC job.
- The fresh sample, which ran first, had 0 failures.

The failed votes were re-called once each (`ic-fill`), as fresh,
independent calls, and are marked `refilled` in the fixture.

| Set | Agreement (3 ensembles) | Ordinary engineering lost | Non-IC voted IC | Decision stable |
| --- | --- | --- | --- | ---: |
| 2026-10-02 (95) | 93 · 93 · 92 | 0 | 0 | 94/95 |
| BRU-330 (50) | 48 · 48 · 49 | 0 | 0 | 50/50 |

Real disagreements remaining (none critical):

- ClickHouse *Database Reliability Engineer*: `unclear` in all three;
- Canonical *Senior SRE / Gitops*: `not` in all three;
- Airbnb *Workday Integration Engineer* (mixed): flips.

**Production consequence:** a vote that fails must be retried or
deferred. It must never silently become `unclear`.

## 6. Phase 2: setup

- **Corpus and candidate:** BRU-325's frozen B1 corpus (wider sources,
  reviewer off), with PR #43's eligibility, at the same instant and for
  the same candidate.
- **What was classified:** all 353 actionable postings, in 3 independent
  ensembles (1,059 calls each, paced; 0 failed votes).
- **Today:** the production selection, replayed:
  - strong fits only, in report order;
  - one company per slot, at most 5;
  - peers ride along with their company's lead.

  The replay reproduces the ungated Today exactly.

## 7. Phase 2: results

The voted decision was identical across ensembles for 349/353 postings.
The three ensembles give **the same Today**.

| | #43 baseline | Gated (all 3 ensembles) |
| --- | ---: | ---: |
| Strong fits | 29 | 26 (25 in one ensemble) |
| Judged Strong yes lost (any tier) | — | **0 of 9** |
| Practical Strong yes (jobs · companies) | 8 · 5 | **8 · 5** |
| Strong tier: judged No | 5 | 3 (Multigres, Railway *Storage*, Axiom *ZK*: depth, out of scope) |
| Today (companies · jobs) | 5 · 12 | 5 · 12 |
| Today: Strong yes · Maybe · No · not in set | 2 · 6 · 2 · 2 | 2 · 7 · 1 · 2 |
| Today: obvious false positives (incl. Wikimedia, post-hoc No) | 3 | 2 |
| Practical Strong-yes companies on Today | 1 | 1 |
| Top-10 plausible: judged No | 6 | 2 |

**Held out of the strong tier:**

- ClickHouse *Senior Curriculum Developer* (No; `not`, a Today lead);
- Railway *Infra Engineer - Datacenters* (No; `unclear`);
- Supabase *FinOps Engineer* (Maybe; `unclear`);
- in one ensemble, Anthropic *Legal Tech* (Maybe; `unclear`).

**Removed from the top-10 plausible:** ClickHouse *Consulting Engineer*,
Oyster *AI Solutions Engineer - GTM*, resend *Customer Success Engineer*
and Canonical *Ubuntu Engineering Lead*. All four are judged No.

**Kept, in every ensemble:**

- PostHog *Product Engineer*;
- Oyster *Senior Engineer (Platform)*;
- Railway *Full-Stack*, *Infrastructure* and *Scalability*;
- Socket *Platform*;
- Supabase *Compute Capacity*, *Supalite* and *Branching*.

**Today, gated:**

| | Job | Judged |
| --- | --- | --- |
| lead | Supabase: Platform Engineer - Compute Capacity + 6 peers | 2 Strong yes, 4 Maybe, 1 No (Multigres) |
| lead | Wikimedia: Senior SWE, Core Experiences (Contract) | not in set; post-hoc No (3-month contract, full-stack web) |
| lead | Sourcegraph: Software Engineer - Platform [IC3] | Maybe |
| lead | Zapier: Staff Backend Engineer, Commerce | not in set; post-hoc Maybe |
| lead | Canonical: Software Developer (Backend SaaS) + MAAS | Maybe, Maybe (replaces Curriculum) |

**Against the success list:**

- Curriculum: gone.
- GTM and Solutions/customer roles: none was left in the strong tier
  after #43; the gate removes them from the plausible list.
- Every good job is kept.

But Today's precision doesn't move. A No lead becomes a Maybe lead, and
the same one practical Strong-yes company is on it.

## 8. What still makes Today wrong

None of these is a function error:

- **Same-company boilerplate:** Supabase's funding and team boilerplate
  lifts 7 peers, 4 of them Maybe and 1 No.
- **Specialization depth:** Multigres (Kubernetes internals); Railway
  *Storage* and Axiom *ZK* in the strong tier.
- **Role shape and engagement:** Wikimedia's 3-month contract.
- **Seniority:** Sourcegraph *IC3*.
- **Company shape:** Canonical (1,200 people, a 2022 posting).

## 9. Cost and latency (measured)

At $0.10 / $0.50 per million input / output tokens:

| | Calls | Cost |
| --- | ---: | ---: |
| Phase 1 (3 × 94 × 3) | 846 | $0.169 |
| Regression (3 × 145 × 3, + refills) | 1,471 | $0.268 |
| Phase 2 (3 × 353 × 3) | 3,177 | $0.651 |
| Smoke test | 6 | $0.001 |
| **Total** | **5,500** | **$1.09** |

- **Per posting version** (3 votes): ≈ $0.0006, about twice the
  single-call function classifier.
- **Call latency:** median 1.7 s, p90 2.6 s.
- **Corpus backfill:** the whole 4,643-posting corpus would cost ≈ $2.80
  once.
- **Rate limits** start after about 10 minutes at concurrency 4. A
  backfill must be paced, and a failed vote retried, never counted.

## 10. Decision

- **Hypothesis, classifier part: confirmed.**
  - A binary question with three votes is accurate on a fresh blind
    sample.
  - It never puts a non-IC job into IC, never loses an ordinary
    engineering job, and is stable at 98.9%.
- **Hypothesis, Today part: not confirmed.** The gate is safe and does
  what it says, but Today is not materially better:
  - strict precision 2/12 → 2/12;
  - practical Strong-yes companies 1 → 1;
  - obvious false positives 3 → 2.

  The classes it removes had mostly already been removed by the
  eligibility patch.

**Recommendation:**

1. **Stop pursuing semantic classifiers as the fix for Today.** Three
   experiments (BRU-325's reviewer, BRU-330, function-only) and this one
   show the same thing. The model reads job *function* well, but function
   is no longer what is wrong with Today.
2. **Next:** the fit and company-evidence architecture. §8 lists the
   concrete failure modes: per-company boilerplate, specialization depth,
   engagement shape, seniority, company shape.
3. **Keep `job-ic/1` frozen and documented**, but don't integrate it. If
   wider sources bring many non-engineering roles back into strong fits,
   it is a cheap, measured guard (≈ $0.0006 per posting version, cached
   globally). Integrating it would need retries for failed votes and a
   paced backfill (§5, §9).

## Reproduce

```bash
cargo build --release -p jobhunt-cli --bin narrow --example job_function_probe
N=target/release/narrow; P=target/release/examples/job_function_probe

# snapshot: BRU-325's 60-board config into a copy of the BRU-324 candidate database
$N --config B-off.toml --database corpus.db find --raw --refresh -v -n 1   # sha256 b5cea521…
# exclude.txt: the 145 ids of bru325-2026-10-01.json and fresh-2026-10-02-job-functions.json
$P sample corpus.db "job-ic/1 2026-10-03" ids.txt selection.json exclude.txt
$P dump   corpus.db ids.txt inputs/
for e in 1 2 3; do OPENAI_API_KEY=… $P ic-run corpus.db ids.txt ens$e.json; OPENAI_API_KEY=… $P ic-fill corpus.db ens$e.json; done
$P ic-score crates/jobhunt-eval/fixtures/real-postings/fresh-2026-10-03-job-ic.json ens1.json ens2.json ens3.json

# Phase 2: the actionable job ids of BRU-325's B1 cell after PR #43 (real_posting_probe),
# classified on that database with the same ic-run/ic-fill; Today replayed from the cell's
# ordered actionable list (strong fits, one company per slot, 5 companies)

# the committed ensembles re-score offline:
cargo test -p jobhunt-eval --test job_ic_runs
```
