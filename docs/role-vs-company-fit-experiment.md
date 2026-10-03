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
