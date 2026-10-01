# Experiment: one explicit target-role question (BRU-324)

Run date: **2026-10-01**. The hypothesis, from
[the BRU-323 teardown](competitive-recommendation-teardown.md#experiment-1-one-explicit-role-question):
Today is empty for the dogfood candidate because Narrow has to *guess* the
kind of work they want; asking once ("What kind of role are you looking
for?") will fill Today with good fits without lowering its bar.

**Falsified if** Today stays empty with a role chosen, **or** strict
precision of what Today shows, judged on real postings, is below 50%.

**Result: failed on precision.** Intent capture improved recall (0 → 13
strong fits, empty Today → 4 companies), but strict precision on the real
Today results is 23% (3 of 13 jobs; 2 of the 4 companies' lead jobs). Role/job
reading and source coverage remain the dominant problem.

## Setup

- **Snapshot**: the BRU-323 run-A2 database (`/tmp/bru323/A2`), the 17
  production boards discovered on 2026-09-30: **2,900 open jobs**. Ranked
  offline (`refresh: never`, `verify: false`), so before and after see the
  identical jobs, verification and eligibility.
- **Candidate**: the BRU-323 dogfood stand-in, the benchmark's synthetic
  senior startup generalist (São Paulo, remote only, no relocation,
  authorized in Brazil, no pay floor), with the A2 description: "Small
  technical teams with high ownership and broad responsibility, at
  startups or growth-stage, engineering-driven companies. I don't want
  early-career roles, engineering management, or large process-heavy
  companies." It names no kind of work, which reproduced the dogfood failure
  (~2,896 open, ~179 actionable, empty Today). The removed backend /
  platform / infrastructure tombstones of A2 were deleted, so the "before"
  profile never stated a kind of work; everything else is A2 as stored.
- **Before**: the BRU-323 release binary (`main`, ranking version 6).
- **After**: this branch's release binary, after one command:
  `narrow preferences roles backend platform infrastructure`. That is the
  persona's own description of the next job in BRU-323 ("senior backend,
  platform or product-infrastructure engineering"), chosen before looking
  at any result. Two other honest answers are reported as sensitivity.
- **Counting**: every open job ranked; Today exactly as the feed builds it
  (strong fits, one company per slot with its other strong fits riding
  along, at most 5 companies). No semantic reviewer, no model.
- **Judging**: the BRU-320 model (Fit: Strong yes / Maybe / No;
  Practicality: Valid / Unknown / Concern / Impossible), one judge, from
  each posting's title, locations and description text, for this persona.
  The labels agree with BRU-323's for the same 13 jobs.

## Funnel

| Metric | Before | After (Backend · Platform · Infrastructure) |
| --- | ---: | ---: |
| Open jobs | 2,900 | 2,900 |
| Ruled out by practicality (remote-only/no-relocation conflict · ineligible) | 2,723 (2,242 · 481) | 2,723 (2,242 · 481) |
| Actionable (eligible or unclear) | 177 | 177 |
| Insufficient | 29 | 22 |
| Plausible (worth reviewing) | 32 | 26 |
| Poor (a material contradiction) | 116 | 116 |
| Strong | 0 | 13 |
| Today companies | 0 | 4 |
| Today jobs (leads + same-company strong fits) | 0 | 13 |
| Strict precision, Today jobs | n/a | 3/13 (23%) |
| Strict precision, Today leads (one per company) | n/a | 2/4 (50%) |
| Relaxed precision (Strong yes + Maybe), Today jobs | n/a | 11/13 (85%) |
| Obvious false positives (No + Impossible), Today jobs | n/a | 2/13 (15%) |

Answering took one command (in the web app, one sheet of chips). Without
an answer the new binary ranks the same snapshot exactly as `main` does
(0 strong, 32 plausible): nothing changes until the person answers.

## Every Today recommendation, judged

| Today | Job | Fit | Practicality | Label | Why |
| --- | --- | --- | --- | --- | --- |
| lead | Supabase: Platform Engineer, Compute Capacity | Strong yes | Valid (Remote, Global) | **Strong yes** | platform/infrastructure, growth stage, "systems you own" |
| + | Supabase: Supalite Engineer | Strong yes | Valid | **Strong yes** | "strong generalist… across the stack", small team, owns large areas |
| + | Supabase: Release Engineer | Maybe | Valid | Maybe | release/SRE shape, on-call heavy; narrower than broad platform |
| + | Supabase: Platform Engineer, Edge & Networking | Maybe | Valid | Maybe | networking specialty (DNS, WAF, CDN, load balancing) |
| + | Supabase: Edge Functions Engineer | Maybe | Valid | Maybe | Rust/Deno runtime work: a runtime specialty |
| + | Supabase: Platform Security Engineer | Maybe | Valid | Maybe | security specialty the persona didn't choose |
| + | Supabase: FinOps Engineer | Maybe | Valid | Maybe | cost engineering with Finance; adjacent, not the work |
| + | Supabase: Multigres Deployment Engineer | **No** | Valid | **No** | "Hands-on experience with Kubernetes internals, custom resources", an operator and CSI drivers |
| lead | Stripe: Backend Engineer, Core Technology | Maybe | **Impossible** (location `US`, posted 2024-06-14) | **Impossible** | read as "remote, no geographic scope": eligibility unclear, not US-only |
| lead | Anthropic: Senior+ Software Engineer, Legal Tech | Maybe | Concern (San Francisco, travel required) | Maybe | a large company the persona avoids; size unread from the posting |
| + | Anthropic: Staff+ Software Engineer, Inference Velocity | Maybe | Concern | Maybe | ML-infrastructure developer productivity at a large company |
| lead | PostHog: Product Engineer | Strong yes | Unknown ("Remote", country USA; PostHog hires across time zones) | **Strong yes** | YC startup, autonomy, product work including the backend |
| + | PostHog: ClickHouse Operations Engineer | Maybe | Unknown | Maybe | specialized database operations |

The checklist: no Early Career, management, GTM, curriculum or training,
solutions architecture, customer support, developer relations, ML
research or database-internals role is among the 13. **Two false positives
are**: a Kubernetes-internals role (Multigres) and
a US-only role (Stripe).

## Sensitivity: other honest answers, same snapshot

| Answer | Strong | Today companies | Today jobs | Strict (jobs) | Strict (leads) | Obvious FP (jobs) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Backend · Platform · Infrastructure (primary) | 13 | 4 | 13 | 3/13 (23%) | 2/4 | 2 |
| Backend · Platform | 10 | 3 | 10 | 3/10 (30%) | 2/3 | 2 (Multigres, Stripe) |
| Backend · Platform · Product engineering | 11 | 4 | 11 | 3/11 (27%) | 2/4 | 2; plus Spotify "Senior SWE, Enterprise AI" (Maybe: a large company they avoid, New York-only listing) |

## What it says

- **Intent was the recall bottleneck.** The same jobs, candidate and
  rules: 0 → 10–13 strong fits, depending only on whether the kind of work
  was said explicitly. The question costs seconds.
- **Precision is not there, and this isn't a threshold problem.** Most of
  the strong tier is one company's specialist roles (networking, runtime,
  security, FinOps, release) read as "platform/infrastructure work, the
  kind you want". Those are role/job-reading errors (specialization depth
  the rules don't see), not missing intent.
- **The two obvious false positives are reading and eligibility gaps
  BRU-323 already named**: a role whose requirements say "Hands-on
  experience with Kubernetes internals, custom resources" that the depth
  rules didn't read as Kubernetes-internals work, and a posting whose
  location is just `US` read as unscoped remote.
- **Coverage**: 4 companies reach Today, one of them contributing 8 of 13
  jobs; only 2 companies offer a practical Strong yes. The universe is still
  too small and US-centric for this person.

## Next experiment

Experiment 2 of BRU-323, now with the intent fixed: freeze a judged
real-posting set from the 17 production boards plus the remote-first boards
(this run's 13 judged jobs are a start), and run it with the existing
semantic reviewer off and on, measuring strict precision of Today on the
same snapshot. Fold in the two deterministic gaps above (a `US`-only
location field is a scope; Kubernetes internals in a role's requirements is
depth) only if the reviewer doesn't remove them.

## Reproduce

```bash
# before: the BRU-323 A2 database, minus its work-shape tombstones
sqlite3 jobhunt.db "delete from profile_taste where dimension='work_shape' and review='removed'"
narrow --config config.toml --database jobhunt.db find --offline --json -n 25
# after: the same file, copied first
narrow --config config.toml --database jobhunt.db preferences roles backend platform infrastructure
narrow --config config.toml --database jobhunt.db find --offline --json -n 25
```

The full funnel (every tier, and Today as the feed selects it) came from a
small read-only program calling `LocalApp::feed` with `refresh: Never`,
`verify: false` on each copy; it isn't part of the repository.
