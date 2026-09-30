# Recommendation-quality invariants

What any change to preferences, ranking or Today must preserve. The
[recommendation benchmark](recommendation-benchmark.md) measures them;
BRU-322 will turn the important ones into hard regression gates.

Narrow tells people that a few of thousands of jobs are *worth their
attention*. That is a precision promise. The standard for earning it:

> A reasonable candidate with this profile would genuinely consider
> applying.

## Invariants

1. **"Worth your attention" requires affirmative evidence.** A job earns
   Today because something shows it fits what the person wants (role
   shape, level, company and team, kind of work), not because nothing
   disproves it. "Not disproven" is not enough.
2. **One weak positive signal is not enough.** A matching keyword, a
   technology the person has used, a remote flag or a fresh posting is
   context. Many of them added together are still context.
3. **Explicit contradictions outrank generic matches.** A seniority,
   role-depth, company-shape or eligibility contradiction keeps a job out
   of Today however many generic matches it has. Detecting a contradiction
   and still surfacing the job is a miss.
4. **Seniority mismatch is material.** An early-career or new-grad role is
   a No for a senior engineer; a senior or staff role is a No for someone
   a year into their career. Both directions count, whatever the stack
   overlap.
5. **Technical keyword overlap is not role-depth fit.** Using PostgreSQL
   is not building a storage engine; running services on Kubernetes is not
   writing its control plane; calling LLM APIs is not training models.
   Deeper specialization needs its own evidence.
6. **Fit and practicality are separate questions.** Fit: would they want
   this company and role? Practicality: can they pursue it (geography,
   authorization, relocation, time zone, pay floor)? A job can be an
   excellent fit and impossible, or practical and a poor fit. Neither
   answer stands in for the other.
7. **Practicality cannot create fit.** Being eligible, remote, recently
   verified or in the right region makes a job possible, not wanted.
8. **High compensation cannot rescue weak fit.** Pay is a practicality. A
   job that qualifies only because of its pay is a false positive.
9. **Unknown compensation does not penalize strong fit.** Unpublished pay
   is unknown, not low; a strong fit with unknown pay still earns Today,
   marked unresolved.
10. **A pay range for another location is not the person's pay.** A
    range labeled for US-based candidates says nothing about pay in
    Brazil: unresolved, never "meets your target".
11. **Unreadable eligibility is not eligibility.** A job whose restriction
    Narrow can't read is unconfirmed; it may be shown as such, but its fit
    must carry it, and it must never outrank confirmed strong fits on
    generic matches.
12. **Fit is the candidate's, not the job's.** The same posting can be a
    Strong yes for one person and a No for another. No rule may classify
    jobs as good or bad in general.
13. **Today is allowed to contain zero jobs.** Zero excellent
    recommendations beat four mediocre ones labeled worth your attention.
    Precision matters more than recall, but a Today that is precise only
    because it is empty is not the goal either: practical Strong yeses
    should surface.

## Checking a change

```bash
cargo run -p jobhunt-eval --bin narrow-eval -- recommendation-benchmark
JOBHUNT_UPDATE_BENCHMARK=1 cargo test -p jobhunt-eval --test recommendation_benchmark
```

The first prints the report; the second regenerates the recorded baseline
(`crates/jobhunt-eval/baseline/recommendation.md`), whose diff shows what a
ranking change did to every case. A change that improves one metric by
emptying Today, or by moving failures from one case to another, shows up
there.
