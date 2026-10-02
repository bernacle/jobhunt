# Experiment: a semantic job-function and specialization classifier (BRU-330)

Run date: **2026-10-02**, on the frozen BRU-325 corpus of 2026-10-01. An
offline measurement: it changes no ranking, threshold, eligibility rule,
source list, reviewer or deployment.

- Annotations:
  [`crates/jobhunt-eval/fixtures/real-postings/bru330-job-classes.json`](../crates/jobhunt-eval/fixtures/real-postings/bru330-job-classes.json).
- Model answers:
  [`bru330-classifier-runs.json`](../crates/jobhunt-eval/fixtures/real-postings/bru330-classifier-runs.json).
- Tooling: [`jobhunt_eval::job_class`](../crates/jobhunt-eval/src/job_class.rs)
  and the probe
  [`crates/jobhunt-cli/examples/job_classifier_probe.rs`](../crates/jobhunt-cli/examples/job_classifier_probe.rs).

## 1. Executive summary

We asked `gpt-6-luna` one narrow question per posting, with no candidate
in the request: *what is this job?* It answered with a function, shapes,
specialization depth and specialties, seniority and customer-facing
nature, each backed by verbatim quotes. Three fresh runs covered the 50
judged BRU-325 postings.

**Function reading works.**

- Function agreement with the annotations was 50/50 in every run. Across
  the three run pairs it was stable on 148 of 150 comparisons.
- All 8 wrong-function jobs were caught in all three runs: Curriculum
  Developer, both Oyster GTM roles, Consulting, Customer Success,
  Integration Engineer, Customer Solution Architect, Platform Architect.
- All 10 Strong-yes-on-fit jobs were read as software engineering ICs in
  all three runs.
- No answer cites company boilerplate. None reasons about an applicant,
  because no applicant is in the request.

**Specialization reading is accurate on clear specialists but not stable
enough.**

- *Specialists:* all 10 deep-specialist critical cases were read as
  specialized or deep in all three runs: Kubernetes operator, ZK, block
  storage, bare metal, edge runtime, database internals, networking.
- *Over-reading:* the model also reads ordinary distributed backend and
  platform work as specialized, tagging `distributed_systems`. 6 of the 10
  Strong-yes jobs are "not general" in at least one run. Railway
  *Infrastructure Engineer*, a Strong yes, is called `deep_specialist` in
  1 of 3 runs, as is Temporal *Software Engineer II*.
- *Stability:* the gate pair is stable on **275/300 field comparisons
  (91.7%)**, against a 95% bar. The 25 material disagreements are mostly
  depth (9), specialties (8) and seniority (5).

**Phase 1 verdict: fail (bar 6, stability).** The disagreements fall on
the axis Narrow would act on to hold specialists back. So Phase 2 (the
integration simulation) was **not run**, and nothing is integrated.

**Cost and latency:**

- The 50-posting run costs ≈ $0.030: about $0.0006 per posting, a third
  less than the old fit reviewer.
- Median latency is 7.3 s per call, about half the old reviewer's.
- Classifying the whole 4,622-posting corpus once would cost ≈ $2.80,
  cached per posting version and shared by every candidate.

**Decision: FAIL** under the gate as specified. In plain terms: function
classification passes every bar; depth and specialty classification
does not. §18 proposes a narrower follow-up.

## 2. The BRU-325 failure being addressed

BRU-325's `ModelFitReviewer` was asked *"is this job unusually right for
this candidate?"* It reasoned like a résumé-gap screener.

- 1 of 8 practical Strong yeses survived.
- It agreed with the judge on 30–40% of reviews.
- A third of identical requests flipped.

Its failure was the task design, not just the model. Given the
candidate's evidence, the model treated any requirement the résumé didn't
show as a reason against the job.

## 3. Hypothesis

Use the model only to *understand the job*: its function, shape, depth,
level and customer contact. Leave "does this person want that kind of
job" to Narrow's deterministic intent, taste and practicality.

- The model never sees a candidate, so it cannot screen one.
- One answer per posting version can be cached for everyone.

## 4. Classifier schema

Strict JSON schema (`response_format: json_schema`, `strict: true`).
The model never outputs a fit, a recommendation or a score.

| Field | Values |
| --- | --- |
| `daily_work` | one sentence, the model's own words (a reading aid; not scored) |
| `function` | `software_engineering_ic`, `engineering_management`, `solutions_or_customer_engineering`, `support_or_success`, `developer_relations`, `curriculum_or_training`, `gtm_or_revenue_engineering`, `product_or_program_management`, `research`, `other_non_engineering`, `unclear` |
| `engineering_shapes` | `backend`, `platform`, `infrastructure`, `product_engineering`, `full_stack`, `frontend`, `mobile`, `developer_tooling`, `sre`, `security`, `data`, `ml_product`, `other` (only for SWE IC) |
| `specialization_depth` | `general`, `specialized`, `deep_specialist`, `unclear` |
| `specialties` | `kubernetes_internals`, `database_internals`, `storage_kernel_io`, `bare_metal`, `networking_edge`, `runtime_compilers`, `cryptography_zk`, `ml_research`, `distributed_systems`, `security`, `developer_productivity`, `observability`, `test_automation`, `performance`, `other` |
| `seniority` | `early_career`, `mid`, `senior`, `staff_plus`, `management`, `unclear` |
| `customer_facing` | `no`, `secondary`, `primary`, `unclear` |
| `evidence` | 1–3 verbatim quotes each for function, shapes, specialization, seniority, customer-facing |

`test_automation` and `performance` were added to the suggested specialty
list. `other` covers the rest.

**Validation** (`job_class::parse`):

- Unknown fields or values reject the answer whole (counted as
  malformed).
- A quote is valid only if, once normalized, it occurs in what was sent:
  title, department and team, or posting text.
- Invalid quotes are dropped and counted.
- A non-unclear field left without a valid quote is reported as
  *unsupported*.

## 5. Prompt and input design

**Input** (`JobInput::build`), the job only:

- **Sent:** the title, department and team, the location fields (as
  description only), and the description's content sentences.
- **Removed:** Narrow's benefits, pay and policy boilerplate filter
  (`facets::content_sentences`), plus "About <company>", "Who we are",
  "Our mission" and similar sections.
- **Never sent:** company name, pay fields, ids, URLs, candidate, taste,
  résumé, preferences, or eligibility logic.
- **Bounded** at 150 sentences and 8,000 characters. The reviewer's
  60-sentence cap cut PostHog *Product Engineer* off before its
  responsibilities: its numbered intro ("1.", "Product-led.") splits into
  many tiny sentences. This was fixed before any model call. Two Oyster
  postings drop one trailing culture sentence; nothing else is truncated.
- **Boilerplate the filter misses** (Oyster's awards, PostHog's "We are",
  Railway's "Things to know", Supabase's "About the team" facts) is still
  sent. The prompt tells the model to ignore it, and no answer quotes it
  (§12).

**Prompt** (`job_class::SYSTEM_PROMPT`, `job-class/1`):

- It defines each value in one line.
- It ranks title and responsibilities above requirements.
- It says that "engineer" in a title doesn't make a job a software
  engineering IC; that technologies taught, sold, supported or integrated
  for customers are not the job's shape; and that depth is about the work
  itself ("high scale", "complex" or "distributed" alone don't make a job
  specialized).
- It calls company text irrelevant and forbids evaluating any applicant.
- It contains no handcrafted per-job rules.

**Leakage risk.** The prompt names job families that occur in the
fixture: revenue operations, solutions, consulting, instructors, Kubernetes
operators, storage engines, ZK proofs, bare metal. They are named
generically, never by title, but the prompt was written knowing this
fixture. Treat the function and deep-specialist results as optimistic
until they are re-measured on a fresh snapshot.

## 6. Evaluation annotations

`bru330-job-classes.json` has one entry per judged BRU-325 posting.

- **Who and when:** one annotator (Claude), from the same bounded text the
  model reads, written **before any model output existed**. Its sha256
  (`74ce43ca…`) was recorded and re-verified after the runs.
- **Scope:** the BRU-325 Fit judgments are untouched; a test asserts the
  two files cover the same 50 jobs in the same order.
- **Format:** each field lists every acceptable answer (rules in the
  file's `conventions`).
  - Shapes are scored only for SWE IC jobs, and must be a non-empty
    subset of the acceptable shapes.
  - Specialties must include at least one required tag (when any are
    listed) and nothing outside the required and allowed tags.
  - Depth for non-engineering work accepts `general`, `specialized` or
    `unclear`.
  - A title with no level accepts `unclear` plus the level its scope
    clearly implies.
- **Critical cases:** 8 bad-function, 1 management, 10 deep-specialist,
  10 Strong-yes-on-fit (the BRU-325 judge's).

## 7. Run 1 results

50 calls: 0 failures, 0 retries, 0 malformed.

| Field | Agreement |
| --- | ---: |
| function | **50/50 (100%)** |
| engineering shapes (SWE IC jobs) | 35/40 (88%) |
| specialization depth | **45/50 (90%)**; SWE-IC jobs 37/42 (88%) |
| specialties | 46/50 (92%) |
| seniority | 50/50 (100%) |
| customer-facing | 45/50 (90%) |
| all six fields | 35/50 |
| quotes | 337 valid, 4 invalid (dropped), 2 fields left unsupported |

## 8. Run 2 results

A fresh run: same requests, no cache. 0 failures, 0 retries, 0 malformed.

| Field | Agreement |
| --- | ---: |
| function | **50/50 (100%)** |
| engineering shapes (SWE IC jobs) | 36/41 (88%) |
| specialization depth | **45/50 (90%)**; SWE-IC jobs 37/42 (88%) |
| specialties | 45/50 (90%) |
| seniority | 50/50 (100%) |
| customer-facing | 45/50 (90%) |
| all six fields | 36/50 |
| quotes | 358 valid, 5 invalid (dropped), 2 fields left unsupported |

**Run 3** (supplementary, fresh) gave function 50/50, shapes 37/41, depth
46/50 (SWE IC 38/42), specialties 46/50, seniority 50/50, customer-facing
44/50, all six 37/50. Quotes: 350 valid, 2 invalid.

## 9. Stability

Per field, two answers are an *exact* match, *materially equivalent* or a
*material* disagreement. The definitions were fixed in `job_class::compare`
before the runs:

- **Shapes:** sets that overlap are equivalent.
- **Depth:** `specialized` vs `deep_specialist` is equivalent ("not
  general").
- **Specialties:** sets that overlap, or both empty, are equivalent.
- **Customer-facing:** `no` vs `secondary` is equivalent.
- **Everything else** that differs is material: function, seniority,
  `general` vs `specialized`, and `unclear` vs any value.

**Gate pair (runs 1 and 2):**

| Field | Exact | Equivalent | Material |
| --- | ---: | ---: | ---: |
| function | 49 | 0 | 1 |
| engineering shapes | 33 | 15 | 2 |
| specialization depth | 39 | 2 | 9 |
| specialties | 36 | 6 | 8 |
| seniority | 45 | 0 | 5 |
| customer-facing | 45 | 5 | 0 |
| **total** | **247** | **28** | **25** |

- **Stable (exact or equivalent): 275/300 = 91.7%. Bar: ≥95%. Not met.**
- Jobs with no material disagreement: 35/50. Identical on all six fields:
  17/50.

**All three pairs:**

| Pair | Stable fields | Jobs without a material disagreement | Material: function · shapes · depth · specialties · seniority · customer-facing |
| --- | ---: | ---: | --- |
| 1–2 (gate) | 275/300 (91.7%) | 35/50 | 1 · 2 · 9 · 8 · 5 · 0 |
| 1–3 | 279/300 (93.0%) | 37/50 | 1 · 2 · 7 · 6 · 5 · 0 |
| 2–3 | 289/300 (96.3%) | 42/50 | 0 · 0 · 4 · 3 · 4 · 0 |

**Every material disagreement in the gate pair** (run 1 → run 2):

| Job | Field: run 1 → run 2 |
| --- | --- |
| Axiom: ZK Proof Engineer | shapes: `backend` → `other` |
| Canonical: Linux Platform Integration | depth: specialized → general; specialties: `other` → none |
| Canonical: Ubuntu Engineering Lead | **function: engineering_management → software_engineering_ic**; shapes: none → `other`; seniority: management → staff_plus |
| ClickHouse: Senior Consulting Engineer | depth: specialized → general; specialties: `other` → none |
| ClickHouse: Senior Curriculum Developer & Instructor | depth: unclear → general |
| Resend: Customer Success Engineer | depth: general → unclear |
| Socket: Senior Platform Engineer *(Strong yes)* | **depth: specialized → general**; specialties: `observability` → none |
| Stripe: Integration Engineer (Metronome) | seniority: senior → unclear |
| Supabase: Customer Solution Architect | depth: unclear → general; seniority: unclear → senior |
| Supabase: Multigres Deployment Engineer | seniority: senior → unclear |
| Supabase: Postgres Deployment Engineer (Nix) | specialties: `developer_productivity` → `other` |
| Supabase: Release Engineer | specialties: `other` → `observability`, `developer_productivity`; seniority: unclear → senior |
| Supabase: Supalite Engineer *(Strong yes)* | **depth: general → specialized**; specialties: none → `other` |
| Temporal: Senior Platform Architect | depth: specialized → general; specialties: `distributed_systems` → none |
| Temporal: Software Engineer II, Open Source Server | depth: general → specialized; specialties: none → `distributed_systems` |

**Where the disagreements fall.**

- **Depth of non-engineering jobs:** 9 of the 25 are depth or specialties
  on jobs that are non-engineering in both runs. They don't matter,
  because the function already routes those jobs.
- **The pre-declared definitions are also too generous in one place.**
  `specialized` vs `deep_specialist` counts as equivalent, but that is the
  boundary a "hold back deep specialists" rule would act on. Railway
  *Infrastructure Engineer* (Strong yes) goes deep → specialized →
  specialized across the runs. That counts as equivalent above but would
  flip a hold-back decision.
- **Post-hoc, decision-relevant view** (computed after the runs; labeled
  as such):
  - function: 49, 49 and 50 of 50 per pair;
  - "deep_specialist or not" on the 42 SWE-IC jobs: 40, 38 and 40 of 42;
  - "general or not": 38, 40 and 40 of 42.

  Function would clear 95%; the depth flags would not.

## 10. Critical cases

Each cell is function · depth · specialties.

#### Bad function

| Job | Run 1 | Run 2 | Run 3 | Result |
| --- | --- | --- | --- | --- |
| ClickHouse: Senior Consulting Engineer - AMER | solutions/customer · specialized · other | solutions/customer · general · — | solutions/customer · general · — | **caught 3/3** |
| ClickHouse: Senior Curriculum Developer & Instructor | curriculum/training · unclear · — | curriculum/training · general · — | curriculum/training · general · — | **caught 3/3** |
| Oyster: Senior AI Solutions Engineer - GTM | GTM/revenue · specialized · other | GTM/revenue · specialized · other | GTM/revenue · specialized · other | **caught 3/3** |
| Oyster: Senior GTM Engineer | GTM/revenue · specialized · other | GTM/revenue · specialized · other | GTM/revenue · specialized · other | **caught 3/3** |
| Resend: Customer Success Engineer | support/success · general · — | support/success · unclear · — | support/success · general · — | **caught 3/3** |
| Stripe: Integration Engineer (Metronome) | solutions/customer · specialized · other | solutions/customer · specialized · other | solutions/customer · unclear · — | **caught 3/3** |
| Supabase: Customer Solution Architect (AMER) | solutions/customer · unclear · — | solutions/customer · general · — | solutions/customer · general · — | **caught 3/3** |
| Temporal: Senior Platform Architect | solutions/customer · specialized · distributed_systems | solutions/customer · general · — | solutions/customer · general · — | **caught 3/3** |

Function quotes are the job's own words, for example:

- "Developing ClickHouse training courses for both on-demand and
  instructor-led delivery";
- "Sitting in Revenue Operations, you'll work alongside GTM Teams";
- "Act as the single owner of customers' scale-proof production
  readiness".

Canonical *Linux Platform Integration* (partner and customer engagement
with OS work) was read as solutions/customer engineering 3/3; the
annotation accepts either function.

#### Management

| Job | Run 1 | Run 2 | Run 3 | Result |
| --- | --- | --- | --- | --- |
| Canonical: Ubuntu Engineering Lead | eng mgmt · general · — | SWE IC · general · — | SWE IC · general · — | management 1/3; staff+ IC lead 2/3 |

The posting itself says the "leadership track includes roles for managers
and Senior+ engineers alike". Both readings are accepted, but it is the
one function flip.

#### Deep specialist

| Job | Run 1 | Run 2 | Run 3 | Result |
| --- | --- | --- | --- | --- |
| Anthropic: Staff+ Software Engineer, Inference Velocity | SWE IC · specialized · developer_productivity, test_automation | SWE IC · specialized · developer_productivity | SWE IC · specialized · developer_productivity | non-general 3/3 (deep 0/3) |
| Axiom: ZK Proof Engineer | SWE IC · deep · cryptography_zk | SWE IC · deep · cryptography_zk | SWE IC · deep · cryptography_zk | non-general 3/3 (deep 3/3) |
| Railway: Infra Engineer - Datacenters | SWE IC · specialized · bare_metal | SWE IC · specialized · bare_metal | SWE IC · specialized · bare_metal | non-general 3/3 (deep 0/3) |
| Railway: Senior Infra Engineer: Baremetal Orchestration | SWE IC · deep · bare_metal | SWE IC · deep · bare_metal | SWE IC · deep · bare_metal | non-general 3/3 (deep 3/3) |
| Railway: Senior Platform Engineer: Storage | SWE IC · deep · storage_kernel_io, distributed_systems | same | same | non-general 3/3 (deep 3/3) |
| Supabase: Edge Functions Engineer | SWE IC · deep · runtime_compilers | same | same | non-general 3/3 (deep 3/3) |
| Supabase: Multigres Deployment Engineer | SWE IC · deep · kubernetes_internals | same | same | non-general 3/3 (deep 3/3) |
| Supabase: Multigres Engineer | SWE IC · deep · database_internals, distributed_systems | same | same | non-general 3/3 (deep 3/3) |
| Supabase: Platform Engineer, Edge & Networking | SWE IC · specialized · networking_edge | same | same | non-general 3/3 (deep 0/3) |
| Temporal: Staff Software Engineer, Traffic | SWE IC · specialized · networking_edge | same | same | non-general 3/3 (deep 0/3) |

Specialization quotes, for example:

- "Maintain our Go-based Kubernetes operator that orchestrates distributed
  Postgres deployments";
- "our new log structured block-storage system";
- "PXE boot, Ansible, and burn-in agents";
- "optimizing ZK provers and implementing novel ZK circuits".

#### Strong yes (BRU-325 Fit)

| Job | Run 1 | Run 2 | Run 3 | Result |
| --- | --- | --- | --- | --- |
| Oyster: Senior Engineer (Platform) | SWE IC · general · — | SWE IC · general · — | SWE IC · general · — | SWE IC 3/3; general 3/3 |
| PostHog: Product Engineer | SWE IC · general · — | SWE IC · general · — | SWE IC · general · — | SWE IC 3/3; general 3/3 |
| Railway: Infrastructure Engineer | SWE IC · **deep** · distributed_systems | SWE IC · specialized · distributed_systems, other | SWE IC · specialized · distributed_systems, performance | SWE IC 3/3; general 0/3; **deep 1/3** |
| Railway: Senior Full-Stack Engineer - Product | SWE IC · general · — | SWE IC · general · — | SWE IC · general · — | SWE IC 3/3; general 3/3 |
| Railway: Senior Product Engineer, Scalability | SWE IC · specialized · distributed_systems, security, performance | SWE IC · specialized · distributed_systems, security, other | SWE IC · specialized · distributed_systems, security | SWE IC 3/3; general 0/3 |
| Socket: Senior Platform Engineer | SWE IC · specialized · observability | SWE IC · general · — | SWE IC · general · — | SWE IC 3/3; general 2/3 |
| Supabase: Platform Engineer - Compute Capacity | SWE IC · specialized · other | same | same | SWE IC 3/3; general 0/3 |
| Supabase: Software Engineer - Branching | SWE IC · specialized · developer_productivity, distributed_systems | same | same | SWE IC 3/3; general 0/3 |
| Supabase: Supalite Engineer | SWE IC · general · — | SWE IC · specialized · other | SWE IC · general · — | SWE IC 3/3; general 2/3 |
| Temporal: Senior SWE, Cloud Platform Foundations | SWE IC · specialized · distributed_systems, performance | same | same | SWE IC 3/3; general 0/3 |

**Every Strong yes is described as a software engineering role**, with
sensible shapes, in every run. None is described through a résumé gap;
there is no résumé in the request.

**But 6 of 10 are read as specialized in at least one run.** The quoted
reasons are the job's own responsibilities: "Develop fraud and abuse
detection", "Enabling continuous regional failover", "orchestrating
ephemeral compute environments", "masters of CPU, Memory, Network and the
Kernel". The model treats ordinary distributed backend and platform work
as a `distributed_systems` specialty. The prompt says "distributed" alone
doesn't make work specialized; the model didn't follow that reliably.

- Supalite's "storage internals" (the BRU-325 reviewer's reason to demote
  it) is **not** what triggers it here; run 2 quotes "Bring more of the
  Supabase stack to SQLite".
- Railway *Infrastructure Engineer* read as `deep_specialist` once is the
  BRU-325 reviewer's misreading again, at a lower rate.

## 11. Exact failures against the annotations

Disagreements per run (field: answer). ✗ marks a run that disagrees.

| Job | Run 1 | Run 2 | Run 3 |
| --- | --- | --- | --- |
| Railway: Senior Product Engineer, Scalability *(Strong yes)* | ✗ depth specialized; specialties | ✗ same | ✗ same |
| Supabase: Software Engineer - Branching *(Strong yes)* | ✗ depth specialized | ✗ same | ✗ same |
| Temporal: SWE, Cloud Platform Foundations *(Strong yes)* | ✗ depth, specialties, shapes (`developer_tooling`) | ✗ depth, specialties, customer-facing | ✗ depth, specialties, shapes, customer-facing |
| Railway: Infrastructure Engineer *(Strong yes)* | ✗ depth **deep_specialist** | — | ✗ specialties (`performance`), customer-facing |
| Socket: Senior Platform Engineer *(Strong yes)* | ✗ depth specialized; shapes (`developer_tooling`) | ✗ shapes (`developer_tooling`) | — |
| Supabase: Supalite Engineer *(Strong yes)* | ✗ shapes (`infrastructure`) | ✗ depth specialized; shapes; specialties | ✗ shapes |
| Temporal: Software Engineer II | — | — | ✗ depth **deep_specialist** |
| Supabase: Edge Functions · Multigres Engineer | ✗ customer-facing `secondary` (open-source collaboration) | ✗ same | ✗ same |
| Supabase: Performance Engineer - Benchmarking | ✗ customer-facing `secondary` | ✗ same | — |
| Supabase: Software Engineer - Auth | ✗ shapes (`product_engineering`) | ✗ same | ✗ same |
| Oyster: Senior Data Engineer | ✗ shapes (`developer_tooling`) | ✗ same | ✗ same |
| Sourcegraph: Platform [IC3] | ✗ specialties (`other`) | ✗ same | ✗ same |
| Canonical: Senior Software Engineer - MAAS | ✗ customer-facing `secondary` | — | — |
| Canonical: Software Developer (Backend SaaS) | — | ✗ customer-facing `secondary` | ✗ same |
| Canonical: Linux Platform Integration | — | ✗ depth general (as solutions/customer) | — |
| Railway: Senior Infra Engineer: Observability | ✗ customer-facing `secondary` | — | — |
| Railway: Infra Engineer - Datacenters | — | ✗ shapes (`sre`) | — |
| Anthropic: Inference Velocity | ✗ specialties (`test_automation`) | — | — |
| Supabase: Compute Capacity *(Strong yes)* | — | — | ✗ customer-facing `secondary` |

The failure classes:

1. **Generalist depth over-read** (the material one): Scalability,
   Branching, Cloud Platform Foundations, Railway Infrastructure, Socket,
   Supalite, Temporal SWE II.
2. **Customer-facing `secondary`** for open-source community or internal
   collaboration. The definition is loose, but nothing material.
3. **Shape tags at the edge** (`developer_tooling`, `product_engineering`,
   `infrastructure` added). Harmless for function, minor for shape.

No failure is a wrong function. No failure comes from candidate reasoning.

## 12. Evidence validity

| | Run 1 | Run 2 | Run 3 |
| --- | ---: | ---: | ---: |
| Valid quotes | 337 | 358 | 350 |
| Invalid (dropped) | 4 (1.2%) | 5 (1.4%) | 2 (0.6%) |
| Fields left with no valid quote | 2 | 2 | 1 |
| Quotes of company boilerplate (funding, founders, awards, team size, "startup", "small team") | **0** | **0** | **0** |

- Every invalid quote is a near-paraphrase of a real sentence, for
  example "Build the infrastructure which powers the Railway engine" for
  "Building the infrastructure which powers the Railway engine…". The
  validation dropped them; none was invented.
- No answer mentions a candidate, applicant, qualification or résumé.

## 13. Tokens, cost and latency (measured)

| | Run 1 | Run 2 | Run 3 |
| --- | ---: | ---: | ---: |
| Classifications · calls | 50 · 50 | 50 · 50 | 50 · 50 |
| Failures · retries · malformed · refused | 0 · 0 · 0 · 0 | 0 · 0 · 0 · 0 | 0 · 0 · 0 · 0 |
| Input tokens (≈1,340 per call are the fixed prompt and schema) | 112,885 | 112,885 | 112,885 |
| Output tokens (including reasoning) | 36,873 | 37,744 | 37,528 |
| Reasoning tokens | 24,659 | 25,356 | 25,129 |
| **Cost** at $0.10 / $0.50 per MTok (price as recorded in BRU-325) | **$0.0297** | **$0.0302** | **$0.0301** |
| Per classification | ≈ $0.0006 | ≈ $0.0006 | ≈ $0.0006 |
| Call latency: median · p90 · max | 7.3 · 10.4 · 14.7 s | 7.3 · 10.5 · 12.1 s | 7.6 · 10.4 · 16.3 s |
| Wall time, 50 postings at concurrency 4 | 99 s | 100 s | 100 s |

- **Total experiment:** ≈ $0.090 for 150 classifications (plus a free
  dry run against a local mock).
- **Versus the BRU-325 reviewer:** ≈ $0.0009 and 14.9 s median per
  review. The classifier is about a third cheaper and about twice as
  fast, because it reads no candidate and asks a smaller question.

## 14. Global-cache economics (estimated)

The classification depends only on the posting version, the classifier
version and the model: `cache_key` = `job-class/1` + `openai:gpt-6-luna` +
the exact input. So the unit is **posting version → classify once → cache
globally**, never posting × candidate.

**Estimate:**

- *Input:* the whole frozen corpus (4,622 open postings) was rendered
  offline. Mean content is 4,838 characters (the sample's: 4,521). With
  the measured 0.202 tokens per character and the fixed prompt, that is
  ≈ 2,320 input tokens per posting.
- *Output:* ≈ 750 tokens per posting, the measured mean.
- *Cost:* ≈ $0.00061 per posting.

| Volume | Cost |
| --- | ---: |
| 4,622 postings, once | ≈ **$2.80** |
| 1,000 new or changed postings per day | ≈ $0.61/day (≈ $18/month) |
| 10,000 new or changed postings per day | ≈ $6.06/day (≈ $182/month) |

- **Wall time:** at the measured ~2 s per posting at concurrency 4, the
  backfill is ≈ 2.5 hours, or minutes at higher concurrency. It is a
  background job, never inline in a Today request.
- **Not measured:** provider-side prompt caching of the fixed ~1,340-token
  prefix could cut input cost further.
- **Comparison:** the per-candidate reviewer scales with candidates ×
  shortlist and re-runs whenever taste changes. This scales with posting
  churn only.

No infrastructure was built.

## 15. Phase 1 verdict

| # | Bar | Result | Met? |
| --- | --- | --- | --- |
| 1 | Every obvious wrong-function job correctly classified | 8/8 bad-function caught in 3/3 runs | **yes** |
| 2 | Deep-specialist roles recognized as specialized or deep | 10/10 non-general in 3/3 runs (6 deep 3/3; Inference Velocity, Datacenters, Edge & Networking, Traffic specialized 3/3) | **yes** |
| 3 | Every known Strong yes described as an appropriate software-engineering role | SWE IC with sensible shapes 10/10 in 3/3 runs; but 6/10 read "specialized" at least once, and Railway *Infrastructure* `deep_specialist` once | **yes on function; depth over-read** |
| 4 | No Strong yes rejected over résumé gaps | No candidate in the request; no answer mentions one | **yes** |
| 5 | ≥ 80% agreement on function and depth | function 100% (3 runs); depth 90 / 90 / 92% | **yes** |
| 6 | ≥ 95% stability across two fresh runs | **91.7%** (gate pair); 93.0% and 96.3% for the other pairs; 35/50 jobs free of material disagreement | **no** |
| 7 | Evidence quotes valid | 98.6–99.4% valid; invalid ones dropped; 0 boilerplate quotes | **yes** |
| 8 | No obvious function-level false positive in the would-be Today set | B1's Today leads ClickHouse *Curriculum* and Oyster *GTM* are non-IC in 3/3 runs; no other B1 Today job is a function-level error | **yes** |

**Phase 1 fails materially on bar 6.**

- **Where the instability falls:** on depth and specialties, the fields
  that were supposed to hold back specialists without losing generalists.
- **What it would cost:** the deep-specialist flag flips on a Strong yes
  (Railway *Infrastructure Engineer*) and on Temporal *SWE II*. Any
  hold-back rule built on it would make the same posting appear or vanish
  depending on which answer was cached first. That is BRU-325's caching
  coin-flip at a lower rate.
- **Bar 3** is met only in the narrow sense; the consistent "specialized"
  reading of three Strong yeses is a second, stable form of the same
  over-reading.

**STOP.** Not integrated; Phase 2 not run.

## 16. Phase 2: integration simulation

**Not run.** The task allows Phase 2 only after Phase 1 passes, and it
didn't.

No deterministic consumer of the classification was written. No ranking,
feed or Today was recomputed with it.

## 17. Comparison against B1 and B2

No simulated comparison exists (see §16). What Phase 1 alone shows about
the BRU-325 cells, without any re-ranking:

| Known BRU-325 problem | B1 (rules) | B2 (old reviewer) | BRU-330 classifier (reading only) |
| --- | --- | --- | --- |
| ClickHouse *Curriculum Developer* | Today lead | removed (poor) | curriculum/training 3/3 |
| Oyster *GTM Engineer* | Today lead | removed (poor) | GTM/revenue 3/3 |
| Temporal *Platform Architect* | strong | held at plausible | solutions/customer 3/3 |
| Multigres *Deployment* (K8s internals) | strong, Today | poor once, plausible once | deep · kubernetes_internals 3/3 |
| Axiom *ZK Proof* | strong, B2 first-load Today lead | poor (steady state) | deep · cryptography_zk 3/3 |
| Railway *Storage* · *Baremetal* | strong | poor · poor | deep 3/3 · deep 3/3 |
| Railway *Infrastructure Engineer* (Strong yes) | strong | **poor** | SWE IC 3/3; **deep 1/3**, specialized 2/3 |
| Supabase *Supalite* (Strong yes) | strong | poor once, plausible once | SWE IC 3/3; general 2/3 |
| PostHog *Product Engineer* (Strong yes) | strong | plausible | SWE IC · general 3/3 |
| Oyster *Platform* (Strong yes) | strong | plausible | SWE IC · general 3/3 |
| Socket *Platform* (Strong yes) | strong | plausible | SWE IC 3/3; general 2/3 |
| Railway *Full-Stack* · *Scalability* (Strong yes) | strong · strong | not reviewed · strong | general 3/3 · specialized 3/3 |
| Supabase *Compute Capacity* (Strong yes) | strong, Today lead | plausible | SWE IC · specialized 3/3 |
| Agreement with manual judgment | — | 30–40% (Fit) | function 100%, depth 90–92% (job annotations; a different question) |
| Stability on identical requests | — | 4 of 12 flipped | function 148/150; all six fields 91.7–96.3% |

The reading is much better than the old reviewer's on exactly the
question it is asked. How many of the 8 practical Strong yeses would
survive integration depends on the deterministic rule. A rule that holds
back only `deep_specialist` keeps all 8 on 2 of 3 runs and loses Railway
*Infrastructure* on the third. A rule that holds back `specialized` too
would lose Compute Capacity, Scalability, Railway Infrastructure and
sometimes Socket and Supalite. That is an estimate, not a measurement.

## 18. Remaining problems

1. **Depth over-reading on distributed generalist work.** `distributed_systems`
   is treated as a specialty whenever the work is distributed. This is the
   main obstacle.
2. **Depth instability at the specialized boundary.** `unclear` ↔
   `general` on non-engineering jobs (harmless), and `general` ↔
   `specialized` ↔ `deep_specialist` on engineering jobs (not harmless).
3. **Seniority `unclear` ↔ `senior`** flips on titles with no level
   (4–5 per pair). The annotation accepts both, but a deterministic
   seniority rule would see a coin flip.
4. **Customer-facing `secondary`** for open-source collaboration and
   internal partners. A definitional looseness, harmless for function.
5. **Ubuntu Engineering Lead** flips between management and staff IC lead.
   The posting is genuinely mixed.
6. **Company boilerplate** still reaches the deterministic ranker
   independently (BRU-325 §18: Supabase funding, Railway "small team"
   lifting every peer). The classifier ignores it; the rules don't. Not
   addressed here.
7. **Eligibility bugs** (US-Remote, bare US, remote with only US or UK
   cities, "based within EMEA", Wikimedia's pay-range exclusion, Zapier
   Americas) are untouched, as specified.
8. **Measurement limits:**
   - one annotator (Claude, the same family as the BRU-325 judge) on one
     snapshot of 50 postings;
   - a prompt written knowing the fixture (§5);
   - one model, three runs.

   Treat agreement figures as ±10 points. The stability shortfall is
   consistent across pairs and runs; the depth over-reading appears in
   all three runs.

## 19. Recommendation

- **Do not integrate this classifier as specified.** In particular, no
  ranking rule should act on `specialization_depth` or `specialties` from
  a single sample.
- **Keep the abstraction.** Job understanding first, deterministic intent
  after: it fixed the BRU-325 reviewer's failure mode. No answer screened
  a candidate, and function reading was right and stable on every hard
  case.
- **Proposed next issue (not created, not implemented):** *function-only
  semantic gate*.
  - Keep `function` and `customer_facing: primary`, which met every bar
    here. Drop depth from the decision path.
  - Re-measure on a **fresh snapshot from a different date** with fresh
    annotations, to control the leakage of §5.
  - Simulate it against B1/B2 (BRU-330 Phase 2 as specified, restricted to
    function).
  - Depth can come back only with a definition that requires the job's
    responsibilities to build a system's internals, and either two
    agreeing samples or a stability bar met on its own.
- **Independent of it:** the three-rule eligibility patch from BRU-325
  §14.

## Final decision

## FAIL — semantic job classification does not generalize reliably

Precisely:

- **Function classification generalizes.** 100% agreement, 148/150
  stable, every wrong-function job caught, every Strong yes read as
  engineering.
- **Specialization depth doesn't, stably.** Two-run stability is 91.7%
  against a 95% bar. Ordinary distributed and platform work is read as
  specialized, and occasionally `deep_specialist`, including a Strong yes.

Per the gate: Phase 2 was not run, and nothing is integrated.

## Reproduce

```bash
cargo build --release -p jobhunt-cli --example job_classifier_probe
P=target/release/examples/job_classifier_probe
DB=corpus-2026-10-01.db   # BRU-325 frozen corpus, sha256 2510768…d224
# ids.txt: the 50 job ids of bru325-2026-10-01.json, one per line

$P dump  $DB ids.txt inputs/           # exactly what each posting sends (no network)
OPENAI_API_KEY=… $P run $DB ids.txt run1.json   # key from the environment only
OPENAI_API_KEY=… $P run $DB ids.txt run2.json   # fresh: no cache between runs
$P score crates/jobhunt-eval/fixtures/real-postings/bru330-job-classes.json run1.json run2.json

# the committed answers re-score offline:
cargo test -p jobhunt-eval --test job_class_annotations
```

The API key came from the process environment of the run commands only.
It was never written to a file, config, log or output (checked: no key
material in any run file), and it is to be revoked after this experiment.
