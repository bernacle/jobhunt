# Experiment: role fit stands on its own; company evidence can't promote weak roles

Run date: **2026-10-03**. *Pre-registration draft, committed before any
frozen-corpus measurement of the new rules. Results are added below it
without editing this part.*

## 1. Current architecture (diagnosis, `fit-rules/2`, ranking v7)

`fit::assess` reads every aspect into one list of affirmative reasons:

- **role:** the shape of work;
- **support:** every other aspect, summed by its best reason each:
  - job-local: seniority, specialization, domain, technology (half),
    work style (half);
  - company: company, team, ownership, culture.

`FitAssessment::classify` makes a job **strong** when all of these hold:

- `role ≥ 0.5`;
- `support ≥ 1.0`;
- `role + support ≥ 2.0`;
- `role ≥ 0.75` or `support ≥ 2.0`;
- no holding contradiction;
- for a person with a firm view on the company side, some company-side
  reason.

Measured on BRU-325's B1 strong tier after PR #43 (29 jobs):

- **Role fit is between 0.75 and 1.0 for every strong job.** The second
  aspect strong needs is almost always company evidence:
  - Supabase: "Over $1B raised (including our $500M Series F)" on all 8
    Supabase strong fits;
  - Railway: "We're a small team, with high ownership" on 7 Railway
    postings;
  - ownership from a heading ("What You'll Own"), from product copy
    ("…to do it all – autonomously"), and from culture boilerplate
    ("values high agency, direct communication, and customer love");
  - "startup" read from an award name (Oyster) and a founder biography
    (Axiom).
- **Company evidence can satisfy the second aspect strong requires**, and
  with `support ≥ 2.0` it can carry a role that only touches the wanted
  work (`role 0.5`).
- **Repeated company text counts once per posting** (the best reason per
  aspect), but it is re-read on every posting of the company, so every
  peer gets the same boost.
- **Unread role-local facts:**
  - levels written "II" or "IC3";
  - company size written "1200+ colleagues";
  - responsibilities written as bullets under "In this role, you'll:".
    The shape is then inferred from scattered words ("from what the
    description asks for").
- **Employment format:** the source's `employment_type` only. A title's
  "(Contract)" is unread. A stated engagement preference is
  eligibility's (the `engagement` rule), not fit's.

## 2. Hypothesis

> Company fit may rank an already-good role higher, but company evidence
> must never be able to make a mediocre role strong. A strong
> recommendation must have enough job-local evidence to stand on its own.

## 3. Evidence taxonomy

| Scope | Aspects (reasons) | Contradictions |
| --- | --- | --- |
| **Role** (job-local) | role (work shape), specialization, seniority, domain, technology, work style, this job (feedback) | work shape (incl. not engineering, team name standing in), role depth, seniority, domain, technology, work style (incl. management) |
| **Company** | company stage/size, team, ownership, culture | company shape, team shape, culture (incl. process-heavy) |

Candidate- and taste-level facts (firmness, what the person wants) stay
as they are. They decide how a fact weighs, never which scope it has.

## 4. The rules (`fit-rules/3`, pre-registered)

**Role fit** (role-scope evidence only) is one of:

- **contradictory**: a material role contradiction;
- **strong**: all of
  1. the work they want, at full strength (`role ≥ 0.75`: the title or
     its specialty, a description sentence about the role, or work a
     product/full-stack role includes);
  2. **evidenced by the job itself**:
     - a responsibility sentence about the role describing that work, or
     - the title naming it plus one more job-local fact: the level they
       want, a specialty they want or have shown, a responsibility
       sentence confirming the title's work, or their own interest in
       the job.

     A shape inferred only from scattered words or a requirements list
     is never enough;
  3. no holding or material role contradiction;
- **plausible**: some of the work they want, or job-local support ≥ 1.5;
- **insufficient**: nothing job-local points to it.

**Company fit** is computed only after role fit:

| Company fit | When |
| --- | --- |
| conflicts | a material or holding company contradiction |
| matches | affirmative company-side reasons |
| unknown | the posting doesn't say |

**Overall level**, in stages; company evidence never appears in the
first one:

| Level | When |
| --- | --- |
| poor | any material contradiction (role or company) |
| strong | role fit strong **and** company fit isn't a conflict. Unknown company isn't negative. |
| plausible | role fit strong with a company conflict, or role fit plausible |
| insufficient | otherwise. Company evidence alone never makes a job plausible. |

**Ordering** among jobs of a level is the existing score: role, plus
job-local support, plus company support, minus held contradictions.
Company fit may rank one strong role above another; it can't make one.

**Readings** (narrow, generic):

- **title levels:** "II", "IC1"–"IC6";
- **company size:** "N colleagues" or "N team members";
- **responsibilities:** bullets under a responsibilities heading or lead
  ("What you'll do", "In this role, you'll:") are sentences about the
  role, for work-shape evidence;
- **contract title:** "(Contract)", "Contractor", "Fixed-term" or "FTC"
  in a title is a contract role when the source is silent.
  - With no engagement preference stated, this is an uncertainty only.
  - The engagement preference itself stays eligibility's; no fit penalty
    is invented.

**Not changed:** sources, eligibility, Today selection (strong fits, one
company per slot, 5 companies, peers ride along), the semantic reviewer
(off), the explicit-role UX. No model is called.

## 5. Pre-registered bar

Measured on BRU-325's frozen B1 corpus with PR #43's eligibility,
reviewer off, the same candidate and instant, against BRU-325's 50
judgments (never relabeled):

- **Today strict precision ≥ 60%** on judged Today jobs, **and** 0 judged
  `No` on Today.
- **All 8 practical Strong yes jobs stay strong**, and no judged Strong
  yes becomes poor. Supabase *Branching* is reported separately.
- **Practical Strong-yes company coverage** stays healthy (baseline 5).

Outcome:

- **PASS:** every bar met, through the separation (not a threshold
  tuned to the fixture).
- **PARTIAL:** the separation works but precision misses because of
  another, identified mechanism.
- **FAIL:** otherwise.

---

# Results (added after the pre-registration)

## 6. Implementation

The reference implementation is on branch
`t3code/role-vs-company-fit-impl`. It is **not proposed for merge** (see
§15).

- **`fit.rs`** (`fit-rules/3`):
  - `Scope` on every aspect and contradiction kind;
  - `RoleFit`, `CompanyFit` and `RoleBasis`;
  - `support` (job-local) and `company_support` kept apart;
  - `classify` in stages: role from role evidence, then the company,
    then the level;
  - `role_reasons()` and `company_reasons()` ("why this role" and "why
    this company");
  - `CompanyBook`: company facts read once per company.
- **`facets`** (the readings):
  - "II" and "IC1"–"IC6" levels;
  - "N colleagues" headcount;
  - duty sentences under a responsibilities heading or lead;
  - a title's responsibility sentence (`ShapeFact::described`);
  - a contract in the title.
- **Ranking:**
  - `RANKING_VERSION` 7 → 8;
  - the batch ranking builds the company book;
  - the cache key includes the company's facts when they change a
    posting's reading.
- **Probe:** `real_posting_probe` prints role fit, company fit, the role
  basis and the role and company reasons separately.

**Changes after the pre-registration**, each made after a measurement:

| # | Change | Why | Effect on the real corpus |
| --- | --- | --- | --- |
| a | A company's own headcount is shared across its postings (`CompanyBook`). First tried as the union of every company trait, then narrowed to the headcount. | The task requires company-scoped evidence. The union spread one posting's "a Fortune 500 CISO" (a customer) to all 48 Supabase postings as "large company", making every Supabase role poor (2 Strong yeses lost). | Canonical (223 of 307 postings state "1000–1200+ colleagues") conflicts with the avoided "large companies" on every posting. |
| b | Strong role strength is categorical (exactly the wanted work, wanted firmly or as read) instead of `role_fit ≥ 0.75`. | A career-inferred (soft, 0.6) want could never make a strong fit: the synthetic `career-inferred` variant fell from 7 Strong yes to 0. | None: the persona's wants are firm. |
| c | A want Narrow only read (soft) needs the work described **and** another job-local fact. | Keeps BRU-324's invariant that an explicit choice surfaces more than career inference (11 vs 10). | None. |

## 7. Frozen-corpus results

Same frozen corpus, candidate, instant and probe as BRU-325 (B1), with PR
#43's eligibility, the reviewer off, and BRU-325's 50 judgments. The two
Today postings outside that set are marked: Wikimedia *Core Experiences
(Contract)* post-hoc No and Zapier *Staff Backend, Commerce* post-hoc
Maybe (not blind).

| | Baseline (`fit-rules/2`) | Pre-registered rules | Final (+ a, b, c) |
| --- | ---: | ---: | ---: |
| Actionable | 353 | 353 | 353 |
| Strong · plausible · insufficient · poor | 29 · 67 · 44 · 213 | 30 · 26 · 34 · 263 | 28 · 23 · 23 · 279 |
| Practical Strong yes (jobs · companies) | 8 · 5 | 8 · 5 | **8 · 5** |
| Today (companies · jobs) | 5 · 12 | 5 · 12 | 5 · 16 |
| **Today strict precision, judged jobs** | **2/10 (20%)** | 2/9 (22%) | **5/14 (36%)** |
| Today strict precision, all jobs | 2/12 | 2/12 | 5/16 |
| Judged No on Today | 2 (+1 post-hoc) | 3 | **3** |
| Practical Strong-yes companies on Today | 1 | 1 | **2** |

Poor rises (213 → 279) because 80 Canonical roles now conflict with the
avoided large company.

## 8. Today, before and after

**Baseline:**

- ClickHouse *Curriculum Developer* (No);
- Supabase ×8: 2 Strong yes, 5 Maybe, 1 No;
- Wikimedia *Core Experiences (Contract)* (post-hoc No);
- Sourcegraph *IC3* (Maybe);
- Zapier *Commerce* (post-hoc Maybe).

**Final:**

| | Job | Judged | Role · company |
| --- | --- | --- | --- |
| lead | Supabase: Platform Engineer - Compute Capacity | Strong yes | strong · matches |
| + | Supabase: Supalite Engineer | Strong yes | strong · matches |
| + | Supabase: Platform Engineer, Edge & Networking | Maybe | strong · matches |
| + | Supabase: Platform Security Engineer | Maybe | strong · matches |
| + | Supabase: FinOps Engineer | Maybe | strong · matches |
| + | Supabase: Multigres Deployment Engineer | **No** | strong · matches |
| lead | Airbnb: Senior Software Engineer, Service Tools | Maybe | strong · unknown |
| + | Airbnb: Senior Fullstack Engineer, Quality Engineering | **No** | strong · unknown |
| lead | Wikimedia: Senior SWE, MediaWiki Content Platform | not in set | strong (title + senior) · unknown |
| lead | Zapier: Staff Backend Engineer, Commerce | post-hoc Maybe | strong · matches |
| lead | Railway: Senior Infra Engineer, Baremetal Orchestration | Maybe | strong · matches |
| + | Railway: Senior Infra Engineer, Observability | Maybe | strong · matches |
| + | Railway: Senior Platform Engineer, Storage | **No** | strong · matches |
| + | Railway: Senior Full-Stack Engineer - Product | Strong yes | strong · matches |
| + | Railway: Infrastructure Engineer | Strong yes | strong · matches |
| + | Railway: Senior Product Engineer, Scalability | Strong yes | strong · matches |

## 9. The nine Strong-yes jobs

| Job | Before → after | Role · company |
| --- | --- | --- |
| Supabase: Compute Capacity | strong → strong | strong (title + senior; described) · matches |
| Supabase: Supalite | strong → strong | strong (duty bullets now read) · matches |
| Railway: Full-Stack - Product | strong → strong | strong (included backend + senior) · matches |
| Railway: Infrastructure Engineer | strong → strong | strong (title confirmed: "As a infrastructure engineer, you will be directly responsible…") · matches |
| Railway: Product Engineer, Scalability | strong → strong | strong ("This is a backend-leaning role…") · matches |
| Socket: Senior Platform Engineer | strong → strong | strong (title + senior) · matches |
| Oyster: Senior Engineer (Platform) | strong → strong | strong (title + senior; described) · matches. Eligibility still uncertain, independently. |
| PostHog: Product Engineer | strong → strong | strong (included backend sentence) · matches |
| Supabase: Branching (reported separately) | plausible → plausible | plausible: a holding seniority contradiction ("3+ years": mid, a step below senior), as before |

No Strong yes changed tier; none is poor.

## 10. Supabase

Every Supabase posting carries the same company evidence: "Over $1B raised
(including our $500M Series F)" and "~400 team members" (48 of 48).

| Job | Judged | Before | Role fit (job-local evidence) | Company fit | After |
| --- | --- | --- | --- | --- | --- |
| Compute Capacity | Strong yes | strong | strong: platform title, senior ("5+ years…") | matches: growth stage, ownership | strong |
| Supalite | Strong yes | strong | strong: "…moving across the stack from API design to storage internals to infrastructure" (duty) | matches: growth, small team, ownership | strong |
| Release Engineer | Maybe | strong | plausible: infrastructure only "from what the description asks for" | matches | **plausible** |
| Edge & Networking | Maybe | strong | strong: platform title, senior | matches | strong |
| Edge Functions | Maybe | strong | plausible: inferred only. Its ownership was the heading "What You'll Own" (company scope now) | matches | **plausible** |
| Platform Security | Maybe | strong | strong: platform title confirmed by a role sentence | matches | strong |
| FinOps | Maybe | strong | strong: "Write the scripts and infrastructure-as-code…" (duty) | matches | strong |
| Multigres Deployment | No | strong | strong: "…building and maintaining the Multigres Operator… Kubernetes-based infrastructure" | matches | strong |
| Branching | Strong yes | plausible | plausible: holding seniority (mid) | matches | plausible |

- **Strong: 8 → 6.**
- **Removed:** the two whose role rested only on the company's funding
  and ownership lines (Release, Edge Functions).
- **Still strong, without the company:** the four Maybes and the No
  have job-local work evidence. The judge's objection is the
  specialization: networking, platform security, cost (FinOps),
  Kubernetes operators. Deterministic depth rules don't read it.

## 11. Company evidence across postings

| Company | Strong before → after | Shared company evidence (postings) | Counted toward strong before | After |
| --- | --- | --- | --- | --- |
| Supabase | 8 → 6 | "Over $1B raised…" (48/48), "~400 team members" (48/48) | yes: the growth-stage reason on all 8 | orders only |
| Railway | 7 → 6 | "we're a startup" (8/8), "We're a small team, with high ownership" (7/8), "This is a high impact, high agency role…" (7/8) | yes: startup, small team, ownership on all 7 | orders only |
| Canonical | 2 → 0 | "…with 1000–1200+ colleagues in 75+ countries" (223/307) | no: unread ("colleagues") | company conflict on every posting |
| PostHog | 2 → 2 | "launched out of Y Combinator" (14/14), "We don't tell anyone what to do" (14/14) | yes | orders only |
| Socket | 1 → 1 | the funding line (28/28), "a strong sense of ownership" (28/28) | yes | orders only |
| Oyster | 1 → 1 | "startup" from an award name, "Greatest Startup Workplaces" (25/25) | yes | orders only |
| Axiom | 2 → 2 | "startup" from a founder biography (3/3), "our small team" (2/3) | yes | orders only |
| Sourcegraph | 1 → 0 | "values high agency, direct communication, and customer love" (10/10) | yes: ownership | orders only. IC3 is now read as mid. |
| ClickHouse | 1 → 0 | "fast-paced startup" (2/193) | yes | orders only; role inferred |

**Before:** each posting re-read the company's boilerplate. Within a
posting a company aspect counted once, but every peer got the same lift,
and that lift could supply the second aspect strong required.

**After:** company evidence can't create a strong role. A company's
stated size is one fact for all its postings. Misread company facts (an
award name, a founder biography) still order strong roles; they don't
make them.

## 12. Nominal cases

- **Wikimedia, *Core Experiences (Contract)*:** strong → plausible.
  - The role rested on backend inferred from a technology list, with no
    responsibility sentence.
  - The title's "(Contract)" is now read (the source gives no employment
    type), and the fit says so: "A contract or fixed-term position, not
    a permanent one".
  - It doesn't change the level: this persona states no engagement
    preference, and the engagement rule is eligibility's. **Unresolved
    product question:** should an unstated engagement preference make a
    short contract a concern?
- **Sourcegraph, *Platform [IC3]*:** strong → plausible. "IC3" is read
  as mid. "A step below the senior level you want" now holds. The
  company's "high agency … customer love" can no longer offset it.
- **Canonical:**
  - Its postings state "1200+ colleagues" (now read, and shared with the
    postings that don't).
  - The persona avoids large companies (confirmed), so it is a material
    company conflict: *Backend SaaS* and *MAAS* go strong → poor.
  - The role fit alone was strong for *Backend SaaS* and plausible for
    *MAAS*.
  - The size is the company's own statement, not inferred from fame.
- **Controls kept strong:** PostHog *Product Engineer*, Oyster
  *Platform*, Socket *Platform*, Railway's three Strong yeses. All on
  job-local evidence (§9).
- **Removed in Railway:** *Infra Engineer - Datacenters* (No): the title
  alone, no level, no responsibility sentence for the infrastructure
  work.

## 13. Synthetic benchmark (BRU-320)

- **Summary unchanged:** strict Today precision 18/18; obvious false
  positives 0; practical Strong yes surfaced 18/18; feed density 12.
- **Row changes:**

| Case | Before → after | Why |
| --- | --- | --- |
| `career-inferred`, `neutral-wins` | 7/11 Strong yes surfaced → 10/11, precision 100% | roles described by the job now carry a read want |
| Tidewater, Fernlight, Kitebrook (No; intent group) | worth reviewing → maybe | company evidence alone no longer makes a job plausible |
| Kestrel *Staff SWE, Product Infrastructure* (Strong yes; excluded below the pay minimum) | strong → worth reviewing | "Product" and "Infrastructure" are only *related* to backend and platform. Its strong fit came from "12-person startup / small team / ownership". The responsibility ("the API, the PostgreSQL database, the deploy pipeline") is backend work the reading doesn't derive from a title that names shapes. **A regression of reading, explained, not relabeled.** |
| Stripe *Payments Infrastructure* (Impossible) | worth reviewing → strong | the role is strong; it stays excluded by eligibility |

- **Integration would need:**
  - the recorded baseline regenerated (`RANKING_VERSION` 8 and the rows
    above);
  - 11 tests rewritten. They assert `fit-rules/2` semantics: a plain
    senior backend role with no company evidence is not strong
    (`preference_controls` ×4, `ranking_e2e`, `rank::tests`, `fit::tests`,
    the semantic-review storage tests ×4).

## 14. Regressions and new risks

- **Unknown company is no longer negative, as the task requires.** So
  strong roles at companies that never state their size or stage become
  strong:
  - Airbnb ×2 (one judged No: *Quality Engineering*, test automation);
  - Anthropic ×3, DoorDash, Automattic, Wikimedia *MediaWiki* (not
    judged).

  Airbnb takes a Today slot. Requiring a company match on top
  (post-hoc, not adopted) gives 6/15 judged on Today.
- **Feed vs single-job view.** The company book exists only when ranking
  many postings. A posting that doesn't state its company's size reads
  differently on its own page (`explain`) than in the feed (Canonical:
  strong alone, poor in the feed). Integration needs a company record
  both paths read.
- **Misread company facts still order.** An award name or a founder
  biography read as "startup" can lift one strong role above another.
- **The duty reading** treats every bullet under a responsibilities lead
  as the role's. A requirements bullet misplaced under it reads as a
  duty (Branching's "Have 3+ years…").

## 15. Conclusion: **PARTIAL**

**The separation is correct and does what it claims:**

- No strong fit rests on company evidence any more. Every remaining
  strong job has job-local evidence for the work (§10–§12).
- The role-local misreadings company evidence used to cover are gone:
  - Curriculum, Release, Edge Functions and the Wikimedia contract
    rested on inferred shapes;
  - Datacenters had a bare title;
  - Sourcegraph is below the wanted level.
- Canonical is held back by its own stated size.
- All 8 practical Strong yes jobs and all 5 companies are kept. Today's
  practical Strong-yes companies go from 1 to 2, and strict precision
  from 2/10 to 5/14.

**It misses the bar:** 36%, against 60% and 0 No. The cause is one
isolated mechanism the separation can't address: **specialization
within the wanted work is unread.**

- **Every remaining No on Today** has real job-local evidence for the
  wanted shape and is a specialty the persona doesn't want:
  - Kubernetes operator internals (Multigres);
  - block storage (Railway *Storage*);
  - test automation (Airbnb *Quality Engineering*).
- **Most Maybes are the same:** Supabase networking, platform security
  and FinOps; Railway bare-metal and observability.
- **The bar is out of reach even with perfect No detection.** A post-hoc
  bound, not a result: dropping every judged No *and* requiring a
  company match yields 7/12 (58%).

**Secondary, measured, not the cause:**

- Today orders verified-first before eligibility-unclear. Socket, Oyster
  and PostHog (strong, eligibility unclear) stay behind other companies.
- Unknown company size admits large companies (§14).

**Recommendation:**

- **Don't integrate `fit-rules/3` yet.** Keep the implementation branch
  as the reference.
- **Next:** one experiment on specialization *within* the wanted shape.
  - Read whether the role is a narrow slice of the work (one subsystem:
    networking, storage, security, cost, CI/release, observability,
    operators) or the broad work itself, for a person who wants broad
    roles.
  - It must be measured on a fresh judged sample, with the nine Strong
    yeses as a guard, and without a growing list of named specialties.
- **If that can't separate the Maybes, reconsider the strict Today
  thesis.** At ≥ 60% strict, Today needs to tell a senior infrastructure
  generalist role from an infrastructure specialist role. Neither the
  rules nor the semantic classifiers have shown they can.
