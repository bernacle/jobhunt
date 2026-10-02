# Experiment: function-only job classification on a fresh snapshot (BRU-330 follow-up)

Run date: **2026-10-02**, on a snapshot fetched that day. An offline
measurement: it changes no ranking, threshold, eligibility rule, source
list, reviewer or deployment.

- Annotations:
  [`crates/jobhunt-eval/fixtures/real-postings/fresh-2026-10-02-job-functions.json`](../crates/jobhunt-eval/fixtures/real-postings/fresh-2026-10-02-job-functions.json).
- Model answers, snapshot and sampling record:
  [`fresh-2026-10-02-function-runs.json`](../crates/jobhunt-eval/fixtures/real-postings/fresh-2026-10-02-function-runs.json).
- Tooling: [`jobhunt_eval::job_function`](../crates/jobhunt-eval/src/job_function.rs)
  and the probe
  [`crates/jobhunt-cli/examples/job_function_probe.rs`](../crates/jobhunt-cli/examples/job_function_probe.rs).
- Offline re-score: `cargo test -p jobhunt-eval --test job_function_runs`.

## Summary

We asked `gpt-6-luna` two questions per posting, with no candidate in the
request: what is this job's function, and is customer contact a defining
part of it. We ran three fresh passes over 95 postings sampled from a new
snapshot.

**Accuracy passes every bar.**

- Function agreement was 94, 95 and 94 of 95.
- All 46 obvious wrong-function jobs stayed out of
  `software_engineering_ic` in all three runs.
- All 33 ordinary engineering jobs were read as `software_engineering_ic`
  in all three runs.
- All 15 customer roles were read as customer-facing `yes` in all three
  runs.
- Customer-facing agreement was 91, 90 and 89 of 95.

**Function stability fails its bar.**

- 91 of 95 postings (95.8%) got the same function in all three runs,
  against a 98% bar.
- Pairwise agreement was 96.8–97.9%, also below 98%.
- Two of the four flips cross the line between engineering IC and
  non-IC. Both are on postings the annotation had already marked as
  genuinely mixed.

**Decision: FAIL.** Phase 2 was not run, and nothing is integrated.

The classifier is cheaper and faster than BRU-330's: ≈ $0.0003 per
posting and 2.9 s median latency. That makes the miss narrow, but it is
still a miss.

## 1. Hypothesis

> AI may be reliable enough to identify the fundamental function of a job
> and whether the role is primarily customer-facing, even if it is not
> reliable enough to judge specialization depth.

BRU-330 measured function at 50/50 agreement and 148/150 pairwise
stability on the 50 BRU-325 postings. Its own §5 flagged that number as
optimistic: the prompt was written knowing that fixture. This experiment
narrows the question to function and customer contact, and re-measures it
on a fresh sample with fresh annotations.

## 2. Fresh snapshot

- **Fetched** in one discovery-only run (`narrow find --raw --refresh`)
  into a copy of the BRU-324 candidate database (`base-candidate.db`, no
  jobs). The run lasted from 2026-10-02 21:01:46Z to 21:02:00Z.
- **Sources:** the same 60-board universe as BRU-325:
  - the 17 production boards of `deploy/cloud.toml`;
  - plus the 43 BRU-323 remote-first boards.

  Source configuration has not changed since BRU-325: no commit touches
  `deploy/` or `jobhunt-sources` after `2d16cf7`.
- **Failures:** 0. `greenhouse:remotecom` answers but lists 0 jobs, as
  it did on 2026-10-01.
- **Open postings:** 4,650, at 58 companies.
- **Frozen:** checkpointed and copied read-only. Its hashes:
  - corpus sha256 `79960e52e65d74771c50ccac07917a6f2d97e961da0a987dd6929e9233a7df8d`;
  - id + content-fingerprint manifest sha256
    `5be74a3cbfc588e39e9eea3db188a654c47bb91a8a1f7b701b5810e5e4e95b92`.

  The postings per source are in the runs fixture.
- **Overlap with BRU-325's corpus** (2026-10-01):
  - 4,557 of the 4,650 ids were already there, 4,492 of them with
    unchanged content;
  - 65 postings of 2026-10-01 have closed.

  A one-day-later snapshot is mostly the same inventory. What makes the
  evaluation set fresh is the sample and its labels (§3), not the
  postings.
- **Committed:** ids, company, title, stratum, labels, model answers and
  quotes of at most 160 characters. No description text is committed.

## 3. Sampling

The sample was drawn by code (`job_function_probe sample`) with a fixed
seed (`job-function/1 2026-10-02`). It was drawn after the classifier was
committed and before any model call, and it was never changed.

**Excluded:** the 50 BRU-325/330 judged postings (49 of them are still
open).

**Strata.** Every open posting goes to the first stratum its lowercased
title matches. Boundary strata come first, so "Solutions Engineer" is
drawn as solutions, not engineering.

| Stratum (title rule) | Pool | Quota | Drawn |
| --- | ---: | ---: | ---: |
| curriculum_or_training | 63 | 4 | 4 |
| gtm_or_revenue | 141 | 5 | 5 |
| developer_relations | 43 | 4 | 4 |
| research | 135 | 5 | 5 |
| product_or_program_management | 273 | 5 | 5 |
| engineering_management | 387 | 6 | 6 |
| solutions_or_architecture | 376 | 7 | 7 |
| support_or_success | 187 | 6 | 6 |
| implementation_integration_consulting | 122 | 5 | 5 |
| platform_in_name_not_engineering_title | 20 | 4 | 3 |
| sre_or_devops | 44 | 4 | 4 |
| security_engineering | 113 | 4 | 4 |
| data_or_ml_engineering | 210 | 5 | 5 |
| mobile | 31 | 3 | 3 |
| frontend | 26 | 3 | 3 |
| full_stack | 53 | 4 | 4 |
| product_engineering | 60 | 4 | 4 |
| platform_engineering | 69 | 4 | 4 |
| infrastructure | 139 | 4 | 4 |
| backend | 82 | 4 | 4 |
| other_engineering_titles | 373 | 3 | 3 |
| other_titles | 1,654 | 3 | 3 |
| **Total** | **4,601** | **98** | **95** |

**How postings are drawn within a stratum:**

- in the order of a seeded hash of the job id;
- one posting per company (two only if the stratum can't fill
  otherwise);
- at most 5 per company overall;
- a company's repeated title is drawn once.

**One procedural fix, made before any model output existed.** The first
draw left the "Platform in name" stratum empty. All its remaining
postings (Stripe, Airbnb, Anthropic) were at companies already at the
cap. The rule was changed so a stratum that can't fill any other way may
exceed the cap. Every other stratum's draw was unchanged. Only 3 such
postings exist outside capped strata, so that stratum has 3.

**The titles are not easy.** Strata are keyword rules, so many draws
land on genuine boundary cases:

- "Forward Deployed Enablement Engineer – Customer Success";
- "Senior Software Engineer, Vehicle Deployment" (deployment of vehicle
  firmware, not of customers);
- "Partner Development Manager" (drawn as management);
- "Enterprise Support Engineer" (the posting's boilerplate says
  "Software Engineer");
- "Workday Integration Engineer" and "Analytics Engineer";
- "Design Engineer (Web & Brand)" in a GTM team;
- "Product Reliability Engineer" in Product Support;
- "Staff Product Marketing Manager, Developer Experience" (drawn as
  product engineering).

## 4. Human annotation

- **Who:** one annotator (Claude), from exactly the bounded text the
  model is sent (`job_function_probe dump`).
- **When:** after sampling and before any model call. The file's sha256
  (`ff6bb091…ffce`) was recorded and re-verified after the runs, and the
  file was committed (`5f1db84`) before the first call.
- **What:** only `function` and `primary_customer_facing`, with the same
  values as the classifier. No specialization, seniority, fit or
  qualification.
- **Mixed postings:** `function_also` and `customer_facing_also` list
  other acceptable answers. They are used on 19 and 14 postings. Strict
  agreement, which ignores them, is reported too.
- **Critical sets**, fixed in the file:
  - `wrong_function`, 46 postings: every posting labelled clearly outside
    software engineering (IC not acceptable, function not `unclear`),
    except Palantir *Security Systems Engineer* (physical security,
    judged borderline).
  - `ordinary_engineering`, 33 postings: clearly a software engineering
    IC job.
  - `customer_role`, 15 postings: solutions, support, implementation,
    onboarding or success work done for customers.

| Function | Postings | | Customer-facing | Postings |
| --- | ---: | --- | --- | ---: |
| software_engineering_ic | 43 | | no | 71 |
| other_non_engineering | 17 | | yes | 23 |
| solutions_or_customer_engineering | 9 | | unclear | 1 |
| support_or_success | 8 | | | |
| engineering_management | 6 | | | |
| product_or_program_management | 5 | | | |
| research | 3 | | | |
| unclear | 2 | | | |
| curriculum_or_training · developer_relations | 1 · 1 | | | |

**Limitation:** one judge, of the same model family as the BRU-325 judge,
on 95 postings. Treat agreement figures as about ±3 points.

## 5. Schema and prompt

The prompt, schema and gates were committed (`0a76fc7`, 21:01:36Z) before
the snapshot was fetched (21:01:46Z) and before the sample existed. The
prompt names generic job families, never titles from any fixture. Every
run carries the digest `c76d364c81cbc4d295a7626aa5fabb87`, and the test
checks it against the code.

**Input.** `JobInput::build`, unchanged from BRU-330:

- **Sent:** the title, the department and team, and the description's
  content sentences, bounded at 150 sentences and 8,000 characters.
- **Removed:** benefits, pay and policy boilerplate, and "About
  &lt;company&gt;" sections.
- **No longer sent:** the location fields, which BRU-330 did send. They
  say nothing about function.
- **Never sent:** the company name, pay, ids, URLs, candidate, taste,
  résumé, preferences, eligibility, or any earlier interpretation.

The input was bounded for 6 of the 95 postings (34 and 41 sentences on
two of them).

**Schema** (strict `json_schema`; evidence comes before each label, so the
model quotes before it decides):

```json
{
  "function_evidence": ["short verbatim quote"],
  "function": "software_engineering_ic | engineering_management | solutions_or_customer_engineering | support_or_success | developer_relations | curriculum_or_training | gtm_or_revenue_engineering | product_or_program_management | research | other_non_engineering | unclear",
  "customer_facing_evidence": ["short verbatim quote"],
  "primary_customer_facing": "yes | no | unclear"
}
```

Nothing else: no depth, specialty, seniority, fit, score or confidence. A
unit test asserts this. Quotes are validated as in BRU-330: a quote not
found in what was sent is dropped and counted.

**System prompt** (`job_function::SYSTEM_PROMPT`, `job-function/1`):

```text
You read one job posting and say what kind of job it is: what the person hired into this role
is primarily expected to do. You never evaluate any applicant: no fit, no qualifications, no
recommendation.

Answer two questions. Before each answer, give one to three short quotes copied exactly from the
title, the department and team, or the posting text (a few words to one clause each, no
ellipses). Quote what the person will do. Give no quotes only when the answer is unclear.

1. function: the job's primary function.
- software_engineering_ic: designs, builds and ships software or systems as an individual
contributor, for the employer's own product, platform or infrastructure (backend, frontend,
mobile, full stack, platform, infrastructure, reliability, security, data, machine learning,
testing, tooling, embedded).
- engineering_management: manages engineers as the main job: direct reports, hiring, leading
teams.
- solutions_or_customer_engineering: technical work for or with particular customers or
prospects: pre-sales and solutions engineering, architects who advise customers, implementing,
integrating or deploying the product for customers, consulting, professional services, field and
forward-deployed engineering.
- support_or_success: resolving customers' issues and tickets, technical support, customer
success and account health.
- developer_relations: advocacy, community and content for outside developers.
- curriculum_or_training: creating or delivering courses, training or certifications.
- gtm_or_revenue_engineering: building or running systems for sales, marketing or revenue
operations (CRM, lead routing, data enrichment, outbound and other go-to-market tooling).
- product_or_program_management: owns product direction, or plans and coordinates programs and
projects, rather than building.
- research: the main output is research (experiments, findings, new models or methods, papers),
not shipped software.
- other_non_engineering: any other job, including sales, marketing, design, data analysis,
finance, legal, people, operations, internal IT support, and hardware, mechanical or electrical
engineering.
- unclear: the posting doesn't say enough, or the job is evenly split between functions.
2. primary_customer_facing: is working directly with customers, prospects or outside users a
defining part of the job?
- yes: the job exists to serve, advise, implement for, support, teach or sell to customers or
outside developers; that contact is central to the daily work.
- no: the work is mainly internal. Occasional customer calls, user research, feedback from users
or open-source community contact don't make it customer-facing.
- unclear.

How to read a posting:
- The title, the department and team, and the core responsibilities outrank the requirements. A
requirement or a technology list never defines the job on its own.
- Technology words don't make a job software engineering. A job that teaches, sells, supports,
implements or integrates a technical product for customers keeps that function however deep the
technology (cloud, Kubernetes, databases, APIs).
- "Engineer", "Architect" or "Developer" in a title doesn't decide the function, and neither
does a team or product name such as "Platform".
- Building internal systems for sales, marketing or revenue teams is gtm_or_revenue_engineering,
even when the work is writing code against APIs.
- A customer-facing architect or engineer is customer-facing whatever the technical domain.
- Ignore company descriptions, mission, funding, growth, awards, founder biographies, culture,
benefits and other employer marketing: never classify from them and never quote them.
- If the primary function or the customer contact is genuinely ambiguous, answer unclear.
```

(Line breaks are reflowed here; the string is in the source.)

**Model and configuration.**

- OpenAI `gpt-6-luna`, the same model as BRU-330, through the same
  adapter: strict `response_format`, no temperature, no effort setting.
- One retry on retryable errors; concurrency 4.
- The key was read from a 0600 file into the run processes' environment
  only. It was never written to the repository, a config or a run file
  (checked), and the file was deleted after the runs.

## 6. Three-run results

Each run was a fresh call per posting: the same 95 inputs, model, schema
and prompt digest, and no cache.

| | Run 1 | Run 2 | Run 3 |
| --- | ---: | ---: | ---: |
| Started (UTC) | 21:21:27 | 21:22:42 | 21:23:53 |
| Answers · failures · retries · malformed | 95 · 0 · 0 · 0 | 95 · 0 · 0 · 0 | 95 · 0 · 0 · 0 |
| Function agreement | **94/95 (98.9%)** | **95/95 (100%)** | **94/95 (98.9%)** |
| Function agreement, strict (first label only) | 91/95 | 92/95 | 90/95 |
| Customer-facing agreement | **91/95 (95.8%)** | **90/95 (94.7%)** | **89/95 (93.7%)** |
| Customer-facing agreement, strict | 91/95 | 89/95 | 88/95 |
| Wrong-function jobs kept out of SWE IC | **46/46** | **46/46** | **46/46** |
| Ordinary engineering jobs read as SWE IC | **33/33** | **33/33** | **33/33** |
| Customer roles read as customer-facing `yes` | **15/15** | **15/15** | **15/15** |
| Non-IC postings read as SWE IC (any class) | **0** | **0** | **0** |
| Valid quotes · invalid (dropped) · labels left unsupported | 353 · 2 · 0 | 345 · 3 · 1 | 352 · 2 · 1 |

## 7. Function agreement

Function was wrong against the annotation once each in runs 1 and 3, and
never in run 2. Both misses confuse one non-IC function with another:

- Zoox *Manager, Technical Program Management – AI*: run 1 said
  `engineering_management` (quoting "Lead an experienced team of TPMs").
  The annotation is program management; runs 2 and 3 agree with it.
- Vanta *Sr. Manager, Security Operations*: run 3 said
  `other_non_engineering` (quoting "Lead and grow a team of the best
  security operations analysts"). The annotation is engineering
  management; runs 1 and 2 agree with it.

Strict misses are answers outside the first label but within the
accepted alternatives:

- Airbnb *Sales Operations Lead*: GTM, 3/3.
- Notion *Commercial Solutions Consultant*: other/sales, 3/3.
- Vercel *Workday Integrations Developer*: other, in 2 runs.
- Linear *Design Engineer (Web & Brand)*: other, in 2 runs.

**Wrong-function cases** were caught in every run. They include every
class the user listed:

- curriculum (Anthropic *BDR Enablement Lead*);
- GTM and revenue (Airbnb *Sales Operations*, Anthropic *Product Marketing,
  GTM*, Render *Revenue Partnerships*);
- solutions, field and forward-deployed engineers (Stripe, Canonical,
  Anthropic, Tailscale, WorkOS, Notion, Figma, Ramp, Palantir);
- support and success (Chainguard, Replit, PlanetScale, Temporal, Vanta,
  Stripe, Palantir *Forward Deployed Enablement Engineer*);
- developer relations (Canonical);
- engineering managers (6);
- research (Anthropic *Interpretability*);
- product and program managers (5);
- "Platform" only as a name (Stripe *Designer, Web Presence & Platform*,
  Stripe *Account Executive, Platforms*);
- engineering-titled non-engineering jobs (GitLab *Security Risk
  Engineer*), and titles that keyword strata misfile (Canonical
  *Engineering Manager – Linux Hardware Enablement*, drawn as curriculum;
  Stripe *Partner Development Manager*, drawn as management).

Each quote is the job's own responsibility, for example:

- "help customers implement…";
- "Respond to and triage incoming support requests";
- "Own the BDR onboarding curriculum";
- "lead our exceptionally talented Security Engineering team".

No non-engineering class was ever read as software engineering.

**Ordinary engineering was never lost** in 99 answers (33 postings × 3
runs). These include:

- postings whose words invite misreading: Zoox *Vehicle Deployment*,
  GitLab *Core DevOps*, Sentry *Billing Platform*, Grafana *Enterprise*
  (whose posting stresses customers), and Figma's *Intern*;
- every backend, platform, infrastructure, full-stack, mobile, product and
  security posting in the sample.

## 8. Customer-facing agreement

All 15 customer roles were read `yes` in all three runs, and no solutions,
support, implementation or success posting was ever read `no`.

The disagreements are the model reading **more** jobs as customer-facing
than the annotation:

| Posting | Annotation | Run 1 · 2 · 3 | Quoted |
| --- | --- | --- | --- |
| Stripe: Compliance Manager, User Enablement | no (or unclear) | yes · yes · yes | "explain intricate compliance requirements to users" |
| Anthropic: Product Marketing Lead, GTM | no (or unclear) | yes · yes · yes | "coordinate workshops and webinars" |
| Anthropic: Community Engagement Manager, Data Centers | no (or unclear) | yes · yes · yes | "Represent Anthropic in public forums, town halls…" |
| Grafana: Staff Backend Engineer – Grafana Enterprise | no (or unclear) | yes · yes · yes | "Engaging directly with large enterprise customers" |
| Grafana: Senior SWE – Session Replay | no | no · yes · yes | "Engage directly with customers – joining calls" |
| Grafana: Staff SWE – Databases SRE | no (or unclear) | no · no · yes | "communication with customers via Bridge calls" |
| Spotify: RecSys 2026 Intern | no (or unclear) | no · unclear · unclear | (no job described) |

- The first three are non-engineering jobs with external audiences
  (users, account champions, local residents). The annotation counted
  only customers. The model is consistent there, so this is a definitional
  difference, not noise.
- **The three Grafana rows are software engineering ICs read as
  customer-facing.** Their function is right in every run. Any future rule
  must therefore never act on `primary_customer_facing` alone. A rule like
  "customer-facing → hold back" would demote real engineering jobs; only
  customer-facing combined with a non-IC function is safe.

## 9. Stability

Primary metric, fixed before the runs: the share of postings answered
identically in **all three** runs.

| | Identical in all 3 runs | Bar | Pairs 1–2 · 1–3 · 2–3 |
| --- | ---: | ---: | --- |
| Function | **91/95 (95.8%)** | ≥ 98% | 92 · 92 · 93 of 95 (96.8 · 96.8 · 97.9%) |
| Customer-facing | **92/95 (96.8%)** | ≥ 95% | 93 · 92 · 94 of 95 |

**Function fails under every reading:** all three runs, any single pair,
and the best pair.

Every instability, without averaging:

| Posting | Field | Run 1 · 2 · 3 | Crosses IC / non-IC? |
| --- | --- | --- | --- |
| Zoox: Manager, Technical Program Management – AI | function | eng mgmt · program mgmt · program mgmt | no |
| Vanta: Sr. Manager, Security Operations | function | eng mgmt · eng mgmt · other | no |
| Vercel: Workday Integrations Developer | function | **SWE IC** · other · other | **yes** |
| Linear: Design Engineer (Web & Brand) | function | other · **SWE IC** · other | **yes** |
| Grafana: Senior SWE – Session Replay | customer-facing | no · yes · yes | (function SWE IC 3/3) |
| Grafana: Staff SWE – Databases SRE | customer-facing | no · no · yes | (function SWE IC 3/3) |
| Spotify: RecSys 2026 Intern | customer-facing | no · unclear · unclear | (function unclear 3/3) |

**Decision-relevant view** (computed after the runs, so labelled
post-hoc). If a consumer only asked "software engineering IC or not", 93
of 95 postings (97.9%) are stable. That is still below 98%.

- The two flips are an internal HR-systems integrations developer and a
  marketing-site design engineer.
- Both were annotated in advance as accepting either reading. Neither
  would plausibly reach this persona's Today.

But the bar is the bar, and the model does not resolve genuinely mixed
postings consistently. It doesn't answer `unclear` for them either.

## 10. Disagreements

Every posting where any run disagrees with the annotation or with
another run:

| # | Posting | Annotation (function / customer-facing) | Run 1 | Run 2 | Run 3 |
| --- | --- | --- | --- | --- | --- |
| 1 | Stripe: Compliance Manager, User Enablement | other / no (unclear) | other / **yes** | other / **yes** | other / **yes** |
| 7 | Anthropic: Product Marketing Lead, GTM | other / no (unclear) | other / **yes** | other / **yes** | other / **yes** |
| 13 | Anthropic: Community Engagement Manager | other / no (unclear) | other / **yes** | other / **yes** | other / **yes** |
| 19 | Zoox: Manager, TPM – AI | program mgmt / no | **eng mgmt** / no | program mgmt / no | program mgmt / no |
| 28 | Vanta: Sr. Manager, Security Operations | eng mgmt / no | eng mgmt / no | eng mgmt / no | **other** / no |
| 47 | Vercel: Workday Integrations Developer | SWE IC (other) / no | SWE IC / no | other / no | other / no |
| 52 | Grafana: Staff SWE – Databases SRE | SWE IC / no (unclear) | SWE IC / no | SWE IC / no | SWE IC / **yes** |
| 69 | Linear: Design Engineer (Web & Brand) | SWE IC (other, GTM) / no | other / no | SWE IC / no | other / no |
| 86 | Grafana: Staff Backend Engineer – Enterprise | SWE IC / no (unclear) | SWE IC / **yes** | SWE IC / **yes** | SWE IC / **yes** |
| 92 | Grafana: Senior SWE – Session Replay | SWE IC / no | SWE IC / no | SWE IC / **yes** | SWE IC / **yes** |
| 93 | Spotify: RecSys 2026 Intern | unclear (research, SWE IC) / no (unclear) | unclear / no | unclear / unclear | unclear / unclear |

Bold marks an answer outside the accepted set. Parenthesised values are
the accepted alternatives.

**Evidence quality.**

- 7 of 1,057 quotes (0.7%) were invalid and dropped. All were
  near-paraphrases or a quote with commentary appended; none was
  invented.
- Two labels were left without a valid quote (customer-facing on Modal
  *Research, Inference* and Airbnb *Staff Data Scientist*).
- No quote comes from company boilerplate. A pattern search found
  "Investor Relations" and "mission-critical" only inside
  responsibilities.
- No answer mentions an applicant.

## 11. Cost and latency (measured)

Price as recorded in BRU-325: $0.10 / $0.50 per million input / output
tokens.

| | Run 1 | Run 2 | Run 3 |
| --- | ---: | ---: | ---: |
| Input tokens (≈ 1,750 per call; about half is the fixed prompt and schema) | 163,532 | 163,532 | 163,532 |
| Output tokens (including reasoning) | 25,517 | 24,818 | 24,978 |
| Reasoning tokens | 16,258 | 15,596 | 15,543 |
| **Cost** | **$0.0291** | **$0.0288** | **$0.0288** |
| Per classification | ≈ $0.00031 | ≈ $0.00030 | ≈ $0.00030 |
| Call latency: median · p90 · max | 2.9 · 4.1 · 6.7 s | 2.8 · 3.8 · 5.3 s | 2.9 · 4.0 · 6.0 s |
| Wall time, 95 postings at concurrency 4 | 74 s | 71 s | 71 s |

- **Total:** ≈ $0.087 for 285 classifications, plus a 2-posting smoke
  test (≈ $0.0007) and a free dry run against a local mock.
- **Versus BRU-330** (7 fields): about half the cost per posting
  ($0.0006 → $0.0003) and less than half the latency (7.3 s → 2.9 s
  median). The smaller schema needs about a third of the output tokens.

## 12. Global-cache economics (estimated; informational, since the gate failed)

The answer depends only on the posting version, the classifier version
and the model. The probe's key is `jfn_` + `job-function/1` +
`openai:gpt-6-luna` + the exact rendered input. The unit is **posting
version → classify once → cache globally**, never posting × candidate.

- **Input:** every one of the 4,650 open postings was rendered offline
  (`job_function_probe size`).
  - mean job text: 4,783 characters;
  - fixed prompt and schema: 4,421 characters;
  - measured ratio: 0.183 tokens per character, plus ≈ 75.

  That is ≈ 1,760 input tokens per posting.
- **Output:** ≈ 264 tokens per posting (measured mean).
- **Cost:** ≈ $0.00031 per posting version.

| Volume | Cost |
| --- | ---: |
| Whole current corpus (4,650 postings), once | ≈ **$1.43** |
| 1,000 new or changed postings per day | ≈ $0.31/day (≈ $9/month) |
| 10,000 new or changed postings per day | ≈ $3.08/day (≈ $92/month) |

- **Wall time:** the backfill is ≈ 1 hour at concurrency 4, and minutes
  at higher concurrency. It would be a background job, never inline in a
  Today request.
- **Churn here:** 158 of 4,650 postings were new or changed in one day.
- **Not measured:** provider-side prompt caching of the fixed prefix.
- **Nothing was built.**

## 13. Phase 1 decision

| Gate | Result | Met? |
| --- | --- | --- |
| Function agreement ≥ 95% in every run | 98.9 · 100 · 98.9% | **yes** |
| 100% of obvious wrong-function jobs kept out of SWE IC | 46/46 in every run | **yes** |
| No non-engineering or customer class systematically read as SWE IC | 0 such answers in 285 | **yes** |
| Every ordinary engineering job read as SWE IC | 33/33 in every run | **yes** |
| Customer-facing agreement ≥ 90% in every run | 95.8 · 94.7 · 93.7% | **yes** |
| No customer role read as non-customer-facing | 15/15 `yes` in every run | **yes** |
| Function stability ≥ 98% | **95.8%** (all three runs identical); pairs 96.8–97.9% | **no** |
| Customer-facing stability ≥ 95% | 96.8% | **yes** |

**FAIL** on function stability. **STOP:** not integrated.

## 14. Phase 2: integration simulation

**Not run.** The task allows Phase 2 only after every Phase 1 gate
passes. No deterministic consumer of the classification was written,
and no ranking, feed or Today was recomputed.

## 15. Baseline vs semantic-function Today

**Not measured**, because it is part of Phase 2.

- The fresh snapshot's baseline Today was not ranked or judged, so there
  is no baseline or semantic-function precision to report from this
  experiment.
- The most recent measured baseline is BRU-325's B1 on 2026-10-01: strict
  precision 3/14 (21%) on Today jobs, with 3 obvious false positives.

## 16. Remaining false positives

None was measured here (see §15). From BRU-325/330, these are the false
positives a function gate would and would not address. They are recorded
so the next experiment starts from them.

- **A function gate would address:**
  - ClickHouse *Curriculum Developer* and Oyster *GTM Engineer* (B1 Today
    leads);
  - Temporal *Platform Architect*.

  This experiment's model reads all of these classes correctly, every
  time, on new postings.
- **Out of scope by design; they would remain:**
  - Kubernetes internals (Multigres Deployment);
  - ZK (Axiom);
  - storage, kernel and bare metal (Railway);
  - deep networking;
  - seniority ambiguity (Temporal *SWE II*, Sourcegraph *IC3*).
- **Practicality errors, not fixed and kept separate:**
  - US-Remote, bare `US`, and US-city-only or UK-city-only lists (Stripe,
    Temporal);
  - "based within EMEA" (Oyster);
  - Wikimedia's US pay-range sentence (a false exclusion);
  - Zapier Americas/NAMER.

  BRU-325 §14's three-rule patch removes all of B2's steady-state false
  positives.

## 17. Conclusion

**FAIL: the fresh snapshot does not confirm function stability.**

- **Accuracy generalizes.** Without having seen these postings, the model
  never put a non-engineering job into engineering. It never lost an
  ordinary engineering job and never missed a customer role. It agreed
  with fresh labels on 98.9–100% of postings. BRU-330's function result
  was not a fixture artifact.
- **Stability does not reach the bar.** 4 of 95 postings change function
  between identical requests:
  - 2 move between two non-IC functions (management vs program
    management, management vs other);
  - 2 move across the IC line, both on genuinely mixed internal-tools
    postings.

  With a global cache, whichever answer is stored first becomes permanent
  for everyone.
- **Customer-facing is not safe on its own.** Grafana engineers who join
  customer calls are read `yes`. Only customer-facing combined with a
  non-IC function would be a safe signal.

**Recommended next action (not created, not implemented):**

1. Ship the independent eligibility patch (BRU-325 §14) first. It is
   deterministic, and the measured Today false positives it removes are
   larger than anything function-gating has been shown to remove.
2. If function gating is pursued, test a version that targets the
   failure mode directly. Measure it on a new snapshot and sample, against
   the same 98% bar:
   - one decision-relevant output: SWE IC or not, with function kept as
     evidence;
   - an explicit "mixed → `unclear`" instruction;
   - self-consistency: 3 votes per posting version, cached. That costs
     about $0.001 per posting, and stability is measured on the voted
     answer.

   A second annotator would tighten the agreement figures.
3. Only after that passes, run Phase 2 as specified: function and
   non-IC-plus-customer-facing as contradictions in deterministic
   ranking, against the BRU-323/324 persona.

## Reproduce

```bash
cargo build --release -p jobhunt-cli --bin narrow --example job_function_probe
N=target/release/narrow; P=target/release/examples/job_function_probe

# snapshot: the 60-board config of BRU-325 (B-off.toml: deploy/cloud.toml's 17 boards +
# the 43 of competitive-recommendation-teardown.md Appendix A), into a copy of BRU-324's
# candidate database with its jobs removed
$N --config B-off.toml --database corpus.db find --raw --refresh -v -n 1
# freeze: checkpoint, copy read-only; sha256 79960e52…df8d

# exclude.txt: the 50 job ids of bru325-2026-10-01.json
$P sample corpus.db "job-function/1 2026-10-02" ids.txt selection.json exclude.txt
$P dump   corpus.db ids.txt inputs/            # exactly what each posting sends (no network)
$P size   corpus.db                            # corpus-wide input size (no network)
for i in 1 2 3; do OPENAI_API_KEY=… $P run corpus.db ids.txt run$i.json; done
$P score crates/jobhunt-eval/fixtures/real-postings/fresh-2026-10-02-job-functions.json \
    run1.json run2.json run3.json

# the committed answers re-score offline:
cargo test -p jobhunt-eval --test job_function_runs
```
