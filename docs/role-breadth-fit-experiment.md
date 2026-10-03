# Experiment: role breadth as a fit concept, without a specialty taxonomy

Run date: **2026-10-03**. *Pre-registration, committed before the fresh
snapshot was fetched, the sample drawn or any breadth logic written.
Results are added below it without editing this part.*

## 1. Starting point

**PR #45** (`docs/role-vs-company-fit-experiment.md`) separated role fit
from company fit. Its reference implementation is
`t3code/role-vs-company-fit-impl`, not merged.

Reproduced today with that branch's binary on BRU-325's frozen B1 corpus
(PR #43 eligibility, reviewer off):

- Today: 5 companies · 16 jobs;
- strict precision on judged Today jobs: **5/14**;
- 3 judged No on Today;
- 8 practical Strong yes jobs, in 5 companies;
- 2 practical Strong-yes companies on Today.

It matches the report exactly.

**Remaining failure:** Narrow reads the role family (platform,
infrastructure, backend) correctly, but not whether the work is the
broad family or one narrow slice of it.

## 2. Phase 0 findings

- **Sound in the reference branch:**
  - the evidence `Scope` (role vs company);
  - the staged `classify` (role from role evidence, then company, then
    level);
  - `support` and `company_support` kept apart;
  - the `RoleBasis` of the role evidence;
  - the narrow readings ("II"/"IC3", duty bullets, a title's
    responsibility sentence).
- **Not to reuse as is:** the cross-posting `CompanyBook`. The feed and
  a single job's page read a posting differently, and it is a
  company-fit mechanism this experiment doesn't need.
- **Existing depth signals:** `facets::work::specialties_in`.
  - It is already a named-specialty taxonomy: 6 specialties (database
    internals, Kubernetes control-plane internals, model training,
    low-latency trading, privacy, security research) and about 100 cue
    phrases.
  - A specialty needs one title cue or two description cues.
- **Why it misses the remaining cases:**
  - its cues are exact phrases ("kubernetes operators", "custom
    controllers"), and Multigres says "the Multigres Operator", "custom
    resources", "Kubernetes internals": one cue;
  - storage, networking, release, cost, observability and test
    automation aren't specialties at all;
  - extending the list is what this experiment must not do.

## 3. Hypothesis

> A candidate-independent reading of how **concentrated** the actual work
> is (broad, focused, deep specialist, or unclear) can be identified
> generically, without naming specialties. Used as one role-local fact,
> it lets a person who wants broad roles keep focused and deep-specialist
> jobs off Today.

## 4. Fresh sample (Phase 1)

- **Snapshot:** one discovery-only fetch of the same 60 boards, into a
  fresh copy of the candidate database, frozen and hashed.
- **Population:** open postings that Narrow's existing deterministic
  facets read as software engineering IC work:
  - function is engineering;
  - not people management;
  - not customer-facing engineering.
- **Excluded:** the 239 postings judged in earlier experiments (BRU-325's
  50, the 2026-10-02 sample's 95, the 2026-10-03 sample's 94).
- **Strata,** first match wins, by the title's own role words:
  - backend, platform, infrastructure, SRE, security, data/ML, developer
    tooling, product, full stack, mobile, frontend, other engineering;
  - crossed with whether the title carries a qualifier after its role
    (", Storage", "- Edge & Networking", ": Observability") or none.

  Qualifier-bearing titles are oversampled, so narrower roles are
  present, but they are not selected by any specialty word.
- **Size:** about 100.
- **Order:** seeded hash of the job id (seed `role-breadth/1 2026-10-03`);
  at most 4 per company; a repeated company + title is drawn once.

## 5. Annotation protocol

- **Who:** one annotator (Claude).
- **What is read:** exactly the job-only text the probe dumps: title,
  department/team, content sentences.
- **When:** before any breadth logic exists. The annotations are
  committed before the first evaluation and never changed afterwards.
- **Labels,** describing the job only, never a candidate:

| Label | Meaning |
| --- | --- |
| `broad` | The responsibilities span several distinct areas of the role family: several systems or services, product areas, end-to-end ownership across a stack or platform. |
| `focused` | Legitimate engineering in the family, but most of the responsibilities center on one narrower technical concern. |
| `deep_specialist` | The work is centered in a narrow subsystem, implementation layer or technical primitive, and needs unusually deep expertise in it. |
| `unclear` | The text doesn't show the breadth of the responsibilities, or it is genuinely mixed. |

- Each label has a short reason and one or two short excerpts.
- Strict agreement uses the label alone; `also` lists an acceptable
  second label for a genuinely borderline job.

## 6. Evidence model (to be implemented after annotation)

Generic properties only. Named specialties can appear as evidence words,
never as categories.

- **Responsibility concentration:** whether the duty sentences (what the
  person will do, not the requirements) keep returning to one concern,
  or span several distinct ones.
- **Title scope:** a qualifier naming a part of the role family. Never
  decisive alone.
- **Implementation layer:** existing depth signals whose work is at a
  lower layer (internals, primitives), from responsibilities rather than
  qualifications.
- **Qualifications are weaker than responsibilities.**
- **Uncertain cases:** prefer `unclear` over narrowing a broad job.

## 7. Phase 1 gate (from the task, unchanged)

`narrow` means `focused` or `deep_specialist`.

| Metric | Definition | Bar |
| --- | --- | --- |
| Narrowness precision | of jobs Narrow marks narrow, share the annotation marks narrow | ≥ 90% |
| Narrowness recall | of jobs the annotation marks narrow, share Narrow marks narrow | ≥ 70% |
| Broad preservation | of jobs annotated `broad`, share Narrow marks `broad` or `unclear` | ≥ 95% |
| Deep cases | every annotated `deep_specialist` | never `broad` |
| Generality | no growing named-specialty list; no systematic family- or company-specific errors | required |

Also reported: the confusion matrix and the `unclear` rate.

**If the gate fails:** stop, report FAIL, integrate nothing, and don't
run Phase 2.

## 8. Phase 2 (only if Phase 1 passes)

- **Setup:** the same frozen corpus and judgments, and the reference
  role/company architecture. Breadth is added as one role-local fact.
- **For a person who prefers broad roles:**
  - `broad` supports the role;
  - `focused` holds a role back from Strong;
  - `deep_specialist` is a material role contradiction;
  - `unclear` is neutral.
- **Company fit can't compensate.**
- **Bar:**
  - Today strict precision ≥ 60%;
  - 0 deterministically rejectable judged No on Today;
  - all 8 practical Strong yes jobs kept, none poor;
  - all 5 companies with practical Strong yes jobs still in the strong
    pool;
  - the improvement comes from the separation and generic breadth
    semantics only.
