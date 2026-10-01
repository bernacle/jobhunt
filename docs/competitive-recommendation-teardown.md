# Competitive teardown: personalized job recommendations (BRU-323)

Research date: **2026-09-30**. This is a research artifact; it changes no
code, ranking or configuration.

The question: why does Narrow's Today still feel materially worse than
Simplify, Welcome to the Jungle/Otta, Wellfound, HiringCafe and similar
products, and what is the smallest evidence-backed change likely to fix it?

Evidence labels, used throughout:

| Label | Meaning |
| --- | --- |
| **[observed]** | Seen directly: a public page, a product's public JS bundle, or a run of Narrow on live job boards |
| **[documented]** | The vendor's own help center, docs, terms or product pages |
| **[community]** | User reports (Reddit, Hacker News, Trustpilot, app stores); the number of independent reports is given, repeated vs anecdote |
| **[inferred]** | Our interpretation |

---

## 1. Executive summary

**The strongest finding.** Today's problem is not the threshold, and it's
not mainly that competitors rank better. It is that Today works from thin
inputs:

1. **No explicit role anchor.** On the same 2,900 jobs and the same
   synthetic persona, Narrow found **13 strong fits when the description
   named the work** ("backend, platform or product-infrastructure") and
   **0 when it named only company and team taste**. The profile-derived
   work shape only reached "worth reviewing", which Today never shows.
   [observed] Every competitor studied anchors recommendations on an
   explicit role picked from a taxonomy (Simplify, WTTJ/Otta, Wellfound,
   YC, Jobright). [documented/observed]
2. **A small, badly aimed universe.** The 17 production boards are mostly
   large, US-centric companies. For a Brazil-based, remote-only
   engineer, **94% of their 2,900 jobs are ruled out** before fit is
   assessed. 8 of the 13 strong fits were at one company (Supabase), and
   only **3 were jobs we'd call practical strong yeses, at 2 companies**.
   [observed] A 5-minute, hand-picked set of 43 remote-first boards gave
   1.8× the eligible jobs from fewer postings, and practical strong yeses
   at 3 other companies. One crafted HiringCafe query found **6 practical
   strong yeses at 6 companies Narrow never reads**. [observed]
3. **The ranker's benchmark precision doesn't carry over to real
   postings.** It scores 100% strict precision on its 27 fixture jobs. We
   hand-judged every job it labeled a "strong fit" on live boards:
   **23% strict precision on the production boards and 26% on the
   expanded boards; 42% obvious false positives on the expanded boards**.
   The #1 "strong fit" there was a *Senior Curriculum Developer &
   Instructor*. [observed; one judge]

**Are competitors better at recommendation quality?** Not demonstrably.
[inferred from observed + community]

- Their matching is mostly taxonomy filtering: role family × level ×
  region × salary floor. Users repeatedly report irrelevant matches,
  ignored preferences and noisy role tags (Simplify, WTTJ, Sonara,
  Jobright, HiringCafe's IC/manager tags).
- For a Brazil-based remote candidate several have essentially nothing:
  - Simplify doesn't support Brazil;
  - WTTJ has no LATAM or worldwide remote option;
  - Jobright and HiringCafe's agent are US-only;
  - Wellfound's remote filter is by company HQ;
  - 27 of 38 YC remote engineering listings were US/Canada-only.
- The HiringCafe sample (search, not a recommender) came out at 30%
  strict precision for the persona. That is comparable to Narrow's real
  strong tier.

Competitors *feel* more productive for four reasons:

- breadth;
- an explicit role anchor;
- company context cards;
- surfaces understood as discovery (feeds, "matches outside your
  preferences", search), so a mediocre item costs little trust.

**Is explicit target-role input important?** Yes. It's the
highest-leverage single question, and it is directly supported: 13 vs 0
strong fits on identical data. [observed]

**Is Narrow's source coverage a major limitation?** Yes, the second
biggest. The universe is too small and pointed at the wrong companies for
the target user. [observed]

**Is company metadata a major limitation?** Secondary.

- Competitors do show far richer company data (size, funding, growth,
  investors, editorial takes). Narrow sometimes reads company stage from
  benefits boilerplate or award text.
- But in our samples most false positives were role-shape, depth and
  eligibility errors, not company-shape errors.
- No competitor models team shape, ownership or engineering culture
  either. [observed/inferred]

**Is Today too strict, too weak, or wrongly framed?** All three, in
different places:

- *Too strict on input.* It needs an affirmative work shape, so with no
  role anchor it shows nothing.
- *Too weak on reading.* When it does fire on real postings, most
  "strong fits" are maybes.
- *Too loose on eligibility.* US-only jobs such as "US-Remote" and
  "Remote (United States)" are read as "unclear", and strong fits with
  unclear eligibility may appear on Today.
- *Framed too strongly.* "N of the 7,500 open jobs Narrow checked are
  worth your attention today", and the web app has no escape hatch when
  that's wrong.

**Recommendation: continue with a narrower change.**

- Ask for the target role explicitly.
- Widen the universe toward companies that hire remotely in Brazil/LATAM.
- Measure precision on real postings rather than fixtures.
- Show the plausible tier, which Narrow already computes, somewhere in
  the web app.
- Don't write another large rules rewrite.

---

## 2. Methodology and limitations

### What was done

1. **Competitor research** (five parallel desk studies, 2026-09-30). Sources:
   - official help centers, product pages, terms and llms.txt;
   - public job and company pages;
   - public JS bundles for Simplify's quiz, WTTJ/Otta's onboarding and
     feed, and YC WaaS's profile wizard;
   - Wayback snapshots where the live site blocked bots (Wellfound);
   - Hacker News (Algolia), Reddit (directly or through the Pullpush
     archive when blocked), Trustpilot, app stores, TabNews (Brazil).
2. **Narrow on live data** (2026-09-30).
   - The release build of this branch, with a throwaway SQLite database
     outside the repo. Nothing in production was touched.
   - The benchmark's synthetic senior startup generalist resume (name
     replaced), described in the persona's words.
   - Practical constraints: based in São Paulo, remote only, no
     relocation, authorized in Brazil, no pay floor (unknown pay is
     acceptable for discovery).
   - Run **A**: the 17 production boards (`deploy/cloud.toml`).
   - Run **B**: 43 remote-first or international-hiring boards (Appendix
     A). Chosen by company reputation *before reading any posting*; the
     slugs were probed for existence only.
   - Run **A2**: the run A jobs with the work-shape statements removed from
     the taste profile, reproducing the dogfood condition.
   - No semantic reviewer (off by default; no model was called).
3. **One competitor sample.** HiringCafe's public search, logged out, in
   a browser located in Brazil. Query "senior backend engineer", with
   its three suggested filters accepted (Software Development, Senior,
   IC) and location set to remote South America. The top 20 results were
   judged.
4. **Judgments** use the BRU-320 framework: Fit (Strong yes / Maybe / No)
   and Practicality (Valid / Unknown / Concern / Impossible). They were
   made from the title, locations, and description excerpts about level,
   scope and location, for the persona in the brief.

### Limitations (read these before quoting numbers)

- **No logged-in competitor feeds.** No accounts were created and no
  personal data was submitted. Simplify Matches, the WTTJ deck, Wellfound
  recommendations, the YC WaaS feed, Sonara and Jobright were studied
  from docs, code, public pages and community reports, not from real
  recommendation lists. Only HiringCafe (public search) has a
  comparable sample, and it is a *search*, not a recommender.
- **One judge, synthetic persona.** All Fit labels are one reader's
  judgment for the brief's persona, not Bruno's. Treat the
  precision figures as ±10–15 points. The direction is not in doubt
  (see the named examples); the exact values are.
- **Small samples.**
  - Narrow's strong tier: A = 13, B = 19. Top-25 lists for context.
  - HiringCafe: 20.
- **The expanded board set is selection-biased on purpose.** It tests
  "does a better-aimed universe help?", not "does a random bigger
  universe help?".
- Reddit was partly blocked. Where Reddit is cited, it came through the
  Pullpush archive or secondary summaries; this is marked.
- Competitor "review" articles are often written by competitors
  (Jobright, Scoutify, Wobo, LoopCV…). They are cited only for facts,
  never for sentiment.
- Wellfound's candidate help articles date from 2023. The Otta-era and
  current WTTJ behavior are separated below.

---

## 3. Competitor matrix

| | **Narrow (today)** | **Simplify** | **WTTJ / Otta** | **Wellfound** | **YC WaaS** (6th) | **HiringCafe** | **Sonara** | **Jobright** (7th, contrast) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Core surface | Today: 0–5 strong fits, one per company | Daily "Job Matches" batch plus job board | One-at-a-time match deck plus themed batches | Search plus alerts plus recruiter inbound | Profile for founder outbound plus job list | Search (72+ filters); agent is US-only | Auto-apply agent | Scored feed plus auto-apply |
| Role anchor | Inferred from free text and profile | Explicit taxonomy, ≤2 | Explicit taxonomy, ≤5 sub-functions | Explicit primary role plus others | Explicit role plus subtype | Query, turned into department, seniority, IC | Titles | Explicit target role |
| Brazil remote | Deterministic eligibility, the best of any studied, with gaps | **Brazil not supported** | No LATAM or worldwide option | Remote filter by company HQ; per-job "Hires remotely in" | Mostly US; about 1 in 38 remote listings open to Brazil | Good structured regions | US-only | US-only |
| Coverage | 17 boards, about 2,900 jobs | Claims 100k+ career sites; about 1.8M sitemap URLs | About 3,500 companies, 70k jobs (UK) | Claims 27k startups | About 1,000 YC companies, about 2,860 eng jobs | Claims 5M+ jobs, 116k+ companies | n/a | Claims 8M+ |
| Company data | Posting text only | Rich: stage, funding, investors, growth, editorial take, grades | Rich: Dealroom funding, growth, "Our take", reviews, DEI | Company-entered stage, size, investors, response-time badges | Batch, team size, founders | Employee count, latest round, investors | n/a | Sponsorship history |
| Feedback | Save / Not for me (with reasons) / Not now / Applied; learned patterns shown | Thumbs, hide job or company; no reasons | Save/Skip; "You skipped lots of X — hide?" | Save, apply | Apply, message | Hide company; Taste Match beta (US) | Thumbs-down (users say it isn't learned) | Not evident |
| Promise | "worth your attention" out of N checked | "Job Matches" | "Matches"; "outside your preferences" when empty | "Recommended at similar companies" | "Companies reach out to you" | Search engine | "Wake up to your best matches" | % match: Strong/Good/Fair |

Why YC WaaS as the sixth product: it's the startup-specific contrast, and
its value is mostly **inbound founder outreach** rather than
recommendations. That's a materially different mechanism, and Narrow
already reads YC job pages. Jobright is included briefly as the purest
"visible match score + volume" design, the opposite of Narrow's.

---

## 4. Onboarding comparison

What each product asks before recommendations appear:

| | Resume | LinkedIn | Target role | Multiple roles | Seniority | Location | Remote | Salary | Company size | Industry | Stage | Tech | Free text | Pref./disliked companies | Work auth / visa | Relocation | Timezone |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| **Narrow** | yes (req.) | optional import | **no: inferred** | inferred | inferred (latest title) | yes | yes | optional | inferred from words | inferred | inferred | from resume | **yes, the main input** | no | yes | yes | optional |
| **Simplify** | optional | no | **req., taxonomy** | ≤2 | req., ≤2 buckets | req. (supported regions) | req. | floor | req. | opt. like/dislike | no | opt. like/dislike | no | later (hide) | profile | no | no |
| **WTTJ (current app)** | optional; CV-to-AI path under an experiment flag | no | **req., taxonomy** | ≤5 | req., ≤2 | yes | yes (fixed regions) | min | yes | like/exclude | no | like/exclude | no (legacy WTTJ: "ideal company") | later (hide) | visa question | no | no |
| **Wellfound** | optional | no | **req. primary role** | yes | years per role | yes | 4-way remote pref | yes, currency | yes | markets like/dislike | no | skills | culture free text | no | US auth only | no | marketing only |
| **YC WaaS** | resume **or** LinkedIn, AI-parsed | yes | **req. role plus subtype** | no | experience | yes | yes | min, if salary matters | yes | no | no | top skills | 3 free-text prompts | no | US visa | yes | no |
| **HiringCafe** | none (agent: yes, US) | no | query | query | filter | IP default | filter | filter | filter | filter | filter | boolean | query | include/exclude filter | visa filter | filter | no |
| **Sonara** | yes | no | titles | yes | no | yes | yes | yes | no | no | no | no | no | no | US only | no | no |
| **Jobright** | yes | no | **target role** | yes | inferred | yes | yes | range | no | inferred | no | inferred | no | no | H1B filter | no | no |

Setup burden:

| Product | Required decisions | Optional | Free text | Time | Burden |
| --- | --- | --- | --- | --- | --- |
| Narrow | resume + 1 description + about 3 practical constraints | review of the reading | 1 (the main input) | about 2–3 min | **very low / low** |
| Simplify | about 7 screens required (values, role, location plus mode, level, size, status, salary) | industries, skills | 0 | about 3–6 min | low–moderate [inferred from observed quiz code] |
| WTTJ | about 15–19 screens (function and level required) | many | 0 | about 5 min | moderate; low on the CV path [observed code; community 2025-04] |
| Wellfound | 4 tabs, about 20–30 fields | many | 1–2 | not observed | moderate [inferred] |
| YC WaaS | about 8 steps, eased by the AI parse | several | 3 | not observed | moderate [inferred] |
| HiringCafe | 0 | 72+ filters | query | seconds to set up, but users poll 3×/day | very low to set up, high to use well [community] |
| Jobright | resume + role, location, remote, type, salary | — | 0 | a few minutes | low [documented] |

Observations:

- **Narrow asks the least structured question of anyone, and the one
  thing it doesn't ask is the thing everyone else requires.** Every
  recommender studied has a *required* role pick. Narrow asks "What kind
  of job are you looking for?" in free text and infers the role from the
  words or the resume. [observed]
- Fewer questions isn't the win. WTTJ asks 15+ screens and is still
  praised. The asymmetry is in *which* question is required.
- Brazil-specific practical questions (country, LATAM, timezone,
  contractor OK) are asked by nobody except Narrow. That's a real
  differentiator for this audience (see §9).

### The explicit target-role hypothesis

**Direct evidence** [observed, runs A and A2, same 2,900 jobs]:

| Taste profile | Eligible | Strong fits (Today candidates) | Worth reviewing (search only) | Today |
| --- | --- | --- | --- | --- |
| Words name the work ("senior backend, platform or product-infrastructure …") | 177 | **13** | 26 | 4 companies |
| Words name only company/team taste ("small technical teams, high ownership, startups …"); work shape left to the profile | 177 | **0** | 32 | **empty: "Nothing worth your time yet"** |

This reproduces the dogfood failure: roughly 2,896 open, 179 actionable,
and nothing on Today. Without a stated work shape the resume-inferred role
counts as *soft*. That makes jobs "worth reviewing" but never "strong",
because fit requires an affirmative role reason plus a second aspect.

**Evaluating the proposed onboarding** (import resume → "What kind of role
are you looking for?" → "Anything else you care about?" → critical
practical constraints → Go):

- It is closest to YC WaaS: resume/LinkedIn parse, role plus subtype,
  three free-text prompts, practical fields. It is shorter than Simplify
  or WTTJ. [inferred]
- The role question should be **a taxonomy with multi-select plus an
  optional free title**, not free text alone:
  - All competitors use a taxonomy, and Narrow already has one
    (`work_shape`: backend, platform, infrastructure, product,
    full_stack, …).
  - A pick is a *firm* statement, which is what the fit rules need.
  - Free text depends on the reader. The built-in reader turned "no ML
    model-training roles" into "**avoid AI**" (the whole domain) in this
    run [observed], so free text alone is fragile.
- It does not recreate the long Preferences form. It's one required
  multi-select.
- **Risk** [inferred]: role taxonomies miss hybrid shapes. Simplify users
  whose job family is missing get nothing (≥4 reports) [community]. So
  keep the free-text title and the "anything else" box.

---

## 5. Candidate-model comparison

E = explicit input, I = inferred, L = learned from behavior, — = not evident.

| Dimension | Narrow | Simplify | WTTJ | Wellfound | YC WaaS | HiringCafe | Jobright |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Skills | I (resume, with evidence) | E + I | E (techs) | E | E (rated) | query | I |
| Titles / past experience | I (evidence graph) | I | I (CV path) | E | I (AI parse) | — | I |
| Seniority | I (latest title) + E if stated | E | E | E (years) | E | E (filter) | I |
| **Desired role** | **I (from words)** | **E** | **E** | **E** | **E** | E (query) | **E** |
| Industries / domain | E (words) | E | E | E (markets) | — | E (filter) | I |
| Company type | E (words) | — | — | E (size) | — | E | — |
| Company size | E (words) | E | E | E | E | E | — |
| Startup stage | E (words) | — | — | — | — | E (round filter) | — |
| Culture / values | E (words) | E (3 values) | E (3 priorities) | E (free text) | E (free text) | — | — |
| Team shape | E (words) | — | — | — | — | — | — |
| Ownership | E (words) | — | — | — | — | — | — |
| Work style / IC vs mgr | E (words) | — (level buckets merge them) | — (merged) | — | — | E (IC/manager filter) | — |
| Salary | E (practical) | E floor | E min | E | E | E (filter) | E |
| Geography / remote | E, deterministic | E (supported regions) | E (fixed regions) | E | E | E | E |
| Feedback | L (with reasons, shown) | L (claimed) | L (save/skip; visible prompts) | — | — | L (Taste Match beta) | — |

[documented/observed; sources in §17]

Narrow models *more* dimensions of taste than anyone: team shape,
ownership, specialization depth, IC vs management. It models *less* of the
one dimension everyone else makes required: the desired role. [inferred]
The richer model only pays off if the job side can be read accurately; §7
shows it often can't yet.

---

## 6. Recommendation-surface comparison

| | Daily set | Infinite feed | Search | Alerts / email | Agent | "Outside your preferences" fallback | Volume |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Narrow (web) | **Today, 0–5** | no | **no (CLI/MCP only)** | email on strong fits | no | **no** | 0–5 |
| Simplify | daily batch, "More Matches" | refreshable | yes | daily/weekly | Talent Agent; Autopilot beta | "Discovery picks outside your usual matches", "Take another look" | unstated; one user got 2/day, another "ran out" [community] |
| WTTJ | deck ("You've seen all your N matches") | batches | no in-app search [observed code] | daily/weekly | no | **"we've found some matches outside of your preferences"** | about 8 relevant/day, per a competitor's review [community, biased] |
| Wellfound | — | — | yes | daily/weekly saved-search alerts | recruiter side | "similar companies" | search-sized |
| YC WaaS | — | — | job list, filters | founder messages | — | — | list-sized |
| HiringCafe | — | paginated | **core product** | saved searches | US-only agent | — | 169 results for the persona query |
| Sonara | "best matches" daily | — | — | — | auto-apply ≤1,000/month | — | volume |
| Jobright | — | scored feed | yes | about 1/day (free) | Orion | — | high |

**Do competitors tolerate mediocre recommendations because users read the
surface as discovery?**

- Yes [inferred, with support]. Everything that recommends also has
  search, batches, or a labeled "outside your preferences" fallback.
  WTTJ and Simplify both make running out of matches explicit and offer
  a broader pool right away. [observed code/documented]
- Community evidence: users praise volume and apply tooling (Jobright),
  or filters and cleanliness (HiringCafe). We found **no user describing
  a feed as a trusted shortlist**, and match labels are mocked when wrong
  (LinkedIn "top applicant"). [community, ≥3 reports]
- Narrow is the only product whose main surface makes a verdict
  ("worth your attention") with no adjacent pool in the web app.
  [observed]

Wording:

| Product | Wording | Strength of claim |
| --- | --- | --- |
| Narrow | "3 of the 7,500 open jobs Narrow checked are worth your attention today"; "Strong fit"; "Nothing worth your time yet" | **strongest**: a verdict out of the whole market |
| Jobright | "92% match"; Strong/Good/Fair Match | strong, but a score invites discounting |
| Simplify | "Job Matches"; "Why this job is a match"; Strong/Fair/Low (resume match) | medium |
| WTTJ | "Matches"; "You're all caught up!"; "matches outside of your preferences" | medium, with a built-in fallback |
| Wellfound | "Recommended jobs at similar companies" | weak |
| HiringCafe | none; "Did I understand you correctly?" | none (search) |

---

## 7. Recommendation-quality samples

Framework: BRU-320. The label is **Impossible** when practicality is
impossible, otherwise the Fit. "Strict" = Strong yes, "relaxed" = Strong
yes + Maybe, "obvious FP" = No + Impossible.

### 7.1 Narrow, run A: the 17 production boards (all 13 strong fits)

Funnel: 2,900 checked → 177 eligible → 61 plausible → 39 worth reviewing
(13 strong). Not shown: 2,242 conflict with remote-only/no relocation,
481 ineligible. [observed]

| # | Job | Fit | Practicality | Label | Reason |
| --- | --- | --- | --- | --- | --- |
| 1 | Supabase: Platform Engineer, Compute Capacity | Strong yes | Valid (Remote, Global) | **Strong yes** | platform, growth-stage, ownership |
| 2 | Supabase: Supalite Engineer | Strong yes | Valid | **Strong yes** | "small team… own large areas end to end" |
| 3 | PostHog: Product Engineer | Strong yes | Unknown (posting says "Remote"; PostHog publicly hires GMT−8…+2) | **Strong yes** | startup, autonomy, product infra |
| 4 | Supabase: Release Engineer | Maybe | Valid | Maybe | release/SRE shape, narrower than broad platform |
| 5 | Supabase: Platform Engineer, Edge & Networking | Maybe | Valid | Maybe | specialization: "4+ years… networking or edge infrastructure" |
| 6 | Supabase: Edge Functions Engineer | Maybe | Valid | Maybe | runtime work, Rust fluency required |
| 7 | Supabase: Platform Security Engineer | Maybe | Valid | Maybe | security specialization |
| 8 | Supabase: FinOps Engineer | Maybe | Valid | Maybe | cost-engineering specialty |
| 9 | PostHog: ClickHouse Operations Engineer | Maybe | Unknown | Maybe | specialized database operations |
| 10 | Anthropic: Senior+ SWE, Legal Tech | Maybe | Concern ("Remote-Friendly (Travel-Required) \| San Francisco") | Maybe | large company, internal tooling |
| 11 | Anthropic: Staff+ SWE, Inference Velocity | Maybe | Concern | Maybe | large company, ML infrastructure |
| 12 | Supabase: Multigres Deployment Engineer | **No** | Valid | **No** | "Hands-on experience with Kubernetes internals, custom resources": the depth rule missed it, and the ranker quoted it as a *positive* "individual-contributor work" reason |
| 13 | Stripe: Backend Engineer, Core Technology | Maybe | **Impossible** ("US-Remote, Chicago, Seattle, San Francisco"; locations `[US]`; posted 2024-06-14) | **Impossible** | eligibility read "US-Remote" as "no geographic scope" |

- **Strict 3/13 (23%) · relaxed 11/13 (85%) · obvious FP 2/13 (15%) ·
  impossible 1/13 · practical strong yes: 3, at 2 companies.**
- Simulated Today (best strong fit per company, ≤5):
  - Supabase Compute Capacity: Strong yes
  - Stripe Core Technology: **Impossible**
  - Anthropic Legal Tech: Maybe
  - PostHog Product Engineer: Strong yes

  That's **2 of 4 strict**.
- Context: the benchmark's own production-shaped validation on this
  snapshot (fit-and-practicality.md) described the same Today. Its
  Supabase and Stripe picks recur here.

### 7.2 Narrow, run B: 43 remote-first boards (all 19 strong fits)

Funnel: 1,779 checked → **326 eligible (18%, against 6% for run A)** → 103
plausible → 71 worth reviewing (19 strong). [observed]

| Job | Fit | Practicality | Label | Reason |
| --- | --- | --- | --- | --- |
| Oyster: Senior Engineer (Platform) | Strong yes | Unknown (UTC−4…+4, country list includes LATAM) | **Strong yes** | platform, growth-stage, IC |
| Railway: Infrastructure Engineer | Strong yes | Valid ("Global"; posted 849 days ago, evergreen) | **Strong yes** | small startup infrastructure |
| Railway: Senior Full-Stack Engineer, Product | Strong yes | Valid (evergreen) | **Strong yes** | product plus backend at a startup |
| Railway: Senior Product Engineer, Scalability | Strong yes | Valid | **Strong yes** | product infrastructure |
| Socket: Senior Platform Engineer | Strong yes | Unknown ("Remote") | **Strong yes** | startup platform |
| Railway: Senior Infra Engineer, Observability | Maybe | Valid | Maybe | observability specialty |
| Railway: Senior Infra Engineer, Baremetal Orchestration | Maybe | Valid | Maybe | bare-metal specialty |
| Railway: Senior Platform Engineer, Storage | Maybe | Valid | Maybe | storage-systems specialty |
| Sourcegraph: Software Engineer, Platform [IC3] | Maybe | Valid (UTC−8…+2) | Maybe | IC3 is one step below senior |
| Canonical: Software Developer (Backend SaaS) | Maybe | Valid (posted 2022-10) | Maybe | 1,200+ people; "1200+ colleagues" is in the posting, unread as size |
| Axiom: Distributed Systems Engineer | Maybe | Unknown (posted 845 days ago) | Maybe | crypto/ZK company, stale |
| **ClickHouse: Senior Curriculum Developer & Instructor** (ranked #1) | **No** | Valid (AMER; regional travel) | **No** | not engineering work. Read as "senior infrastructure work (from what the description asks for)"; startup from "a fast-paced startup environment" |
| **Oyster: Senior GTM Engineer** (ranked #2) | **No** | Concern (Spain, Romania, Portugal, Slovakia, Poland; read as "hires… from anywhere") | **No** | revenue-ops work read as backend; "startup" from an award name |
| Axiom: ZK Proof Engineer | **No** | Unknown | **No** | deep cryptography specialty, no depth rule |
| Railway: Infra Engineer, Datacenters | No | **Impossible** ("Remote (United States)", read as unclear) | **Impossible** | physical datacenter work, US-only |
| Temporal: Senior SWE, Cloud Platform Foundations | Strong yes | **Impossible** (Seattle/Austin/SF) | **Impossible** | — |
| Temporal: Staff SWE, Traffic | Maybe | **Impossible** (Seattle) | **Impossible** | — |
| Temporal: SWE II, Open Source Server | Maybe | **Impossible** (US) | **Impossible** | — |
| Temporal: Senior Platform Architect | No | **Impossible** (London/Manchester) | **Impossible** | customer-facing architect |

- **Strict 5/19 (26%) · relaxed 11/19 (58%) · obvious FP 8/19 (42%) ·
  impossible 5/19 · practical strong yes: 5, at 3 companies.**
- Simulated Today, as the CLI ordered it:
  - ClickHouse Curriculum Developer: **No**
  - Oyster GTM Engineer: **No** (Oyster Platform rides along as a peer)
  - Sourcegraph Platform IC3: Maybe
  - Canonical Backend SaaS: Maybe

  That's **0 of 4 strict, 2 of 4 obvious false positives.**
- These numbers are close to the **pre-BRU-322 baseline** (49% strict,
  43% FP on fixtures). On postings the rules weren't written against,
  v6's precision regressed to roughly v5's. [observed; one judge]

### 7.3 HiringCafe: top 20 for the persona (search, 2026-09-30)

Query: "senior backend engineer" + Software Development + Senior + IC +
remote in South America → 169 jobs, 102 companies. [observed]

| # | Job | Fit | Practicality | Label |
| --- | --- | --- | --- | --- |
| 1 | RevenueCat: Senior Backend Engineer ($230k; Americas/EU/…) | Strong yes | Valid | **Strong yes** |
| 2 | Commit (IT services): Senior Backend (Buenos Aires) | No (agency) | Impossible (Argentina) | Impossible |
| 3 | Bunch: Senior Backend (TypeScript/Nest; Brazil) | Strong yes | Valid | **Strong yes** |
| 4 | TRACTIAN: Senior Backend (Brazil; Go/Node; 3 months old) | Strong yes | Valid | **Strong yes** |
| 5 | Bluehost: Senior Backend (Python/RAG; Brazil) | No (large company) | Valid | No |
| 6 | Skeelo: Senior Backend (Node; Brazil, domestic) | Maybe | Valid | Maybe |
| 7 | KAST: Senior Backend (Go; Brazil; startup) | Strong yes | Valid | **Strong yes** |
| 8 | Nango: Backend Engineer, Senior/Staff ($120–200k; incl. Brazil) | Strong yes | Valid | **Strong yes** |
| 9 | Kiwi Financial: Backend Engineer (Colombia, Argentina, Mexico) | Strong yes | Impossible | Impossible |
| 10 | Klar: Senior Backend, Germany (Java) | Maybe | Impossible | Impossible |
| 11 | OX Security: Senior Backend (Node/TS; Argentina) | Strong yes | Impossible | Impossible |
| 12 | Monks (agency): Backend (Colombia) | No | Impossible | Impossible |
| 13 | Mimica: Staff/Senior Backend (Node; incl. Brazil) | Strong yes | Valid | **Strong yes** |
| 14 | Mode Mobile: Sr Backend (VPN/networking; incl. São Paulo) | Maybe | Valid | Maybe |
| 15 | Uberall: Senior Backend (Kotlin; Brazil/Argentina; contract) | Maybe | Valid | Maybe |
| 16 | Archy: Senior Backend SE (South America/Brazil) | Maybe | Valid | Maybe |
| 17 | (xFarm) Java Backend Developer (Brazil, R$14–16k/mo) | Maybe | Valid | Maybe |
| 18 | Quartile: Senior Backend (.NET; Brazil; contract) | Maybe | Valid | Maybe |
| 19 | Wellhub: Senior Backend, AI Engine (Brazil) | No (large company) | Valid | No |
| 20 | Intuition Machines: Senior Backend, Security (Argentina) | Maybe | Impossible | Impossible |

- **Strict 6/20 (30%) · relaxed 12/20 (60%) · obvious FP 8/20 (40%) ·
  impossible 6/20 · practical strong yes: 6, at 6 companies.**
- All 6 impossible results come from the region filter the persona chose
  ("South America" ≠ Brazil). The agent's run with Brazil plus
  unrestricted South America/Worldwide found all top-15 valid for Brazil
  [observed]. So HiringCafe's geography model is good. The noise is fit,
  and the user fixes it by reading the results.
- **None of the 6 practical strong-yes companies is in Narrow's source
  set.**

### 7.4 Comparison

| | Sample | Strict | Relaxed | Obvious FP | Impossible | Practical strong yes (companies) |
| --- | --- | --- | --- | --- | --- | --- |
| Narrow benchmark (fixtures, v6) | 18 surfaced | 100% | 100% | 0% | 0 | 18 |
| Narrow run A strong tier (17 boards) | 13 | 23% | 85% | 15% | 1 | 3 (2) |
| Narrow run B strong tier (43 remote-first boards) | 19 | 26% | 58% | 42% | 5 | 5 (3) |
| HiringCafe search, top 20 | 20 | 30% | 60% | 40% | 6 | 6 (6) |
| Simplify / WTTJ / Wellfound / YC / Jobright | no comparable sample | — | — | — | — | — |

What we can say:

- On real postings, Narrow's "strong fit" is about as precise as a
  well-crafted search, not the near-certain verdict its wording claims.
  [observed]
- The search's advantage is coverage: 6 distinct right companies in 20
  results, against 2–3 in Narrow's whole strong tier. [observed]
- For the logged-in recommenders there's no comparable sample. Community
  evidence points to precision no better than this (role tags wrong,
  preferences ignored), but that is not measured. [community/inferred]

---

## 8. Job and company data comparison

### 8.1 Job coverage: "are we looking for a perfect job in too small a universe?"

**Yes.** [observed]

- The 17 production boards are Linear, Ramp, Notion, Supabase, PostHog,
  Modal, Replit, Vanta, Anthropic, Stripe, Figma, Airbnb, Spotify,
  Palantir, Zoox, and DoorDash plus PostHog via YC.
- For this persona, **2,723 of 2,900 jobs (94%) are ruled out**
  before fit: 2,242 conflict with remote-only/no relocation, 481 are
  ineligible.
- Of the rest, strong fits came from **5 companies**, and practical
  strong yeses from **2** (Supabase, PostHog).
- Today's one-per-company rule then leaves at most about 3–4 items,
  whatever the ranker does.

The 43-board remote-first set:

- It turned 1,779 jobs into **326 eligible**: 1.8× the eligible jobs from
  61% of the postings.
- It found practical strong yeses at 3 new companies (Railway, Oyster,
  Socket).
- Its strong tier also contained plausible jobs at Sourcegraph and
  Canonical.

HiringCafe, for one query:

- 169 jobs at 102 companies for this persona.
- 6 practical strong yeses in its first 20, all at companies Narrow
  doesn't read.
- It indexes company career pages across ATSs (Greenhouse, Lever, Ashby,
  Workday, Workable…) and claims 116k+ companies. [documented]

What competitors do:

| | Model | Startup coverage | International remote | Staleness |
| --- | --- | --- | --- | --- |
| Simplify | scrapes career sites ("100,000+") | curated startup lists | 10 supported countries; no Brazil | "Confirmed live in the last 24 hours" |
| WTTJ | curated plus crawled; now includes Amazon etc. | historically strong, reportedly declining post-rebrand [community, 3] | UK/US/CA/EU | "We check jobs are live every few hours" |
| Wellfound | company-posted | 27k startups (claimed), plus public companies | per-job "Hires remotely in" | "Reposted 11 months ago" labels |
| YC WaaS | founder-posted, YC only | about 1,000 companies | mostly US | 22 of 38 remote SWE postings older than about 2 months [observed] |
| HiringCafe | ATS/career-page scraping plus LLM extraction | broad | structured regions | expired-job complaints repeated but not dominant [community, 2 threads] |

- **The biggest structural gap is aim, not size.** Narrow's
  first-party-ATS approach is the same one HiringCafe and Simplify use
  and that users praise ("no reposts, no agencies"). It points at the
  wrong 17 companies for a remote-from-Brazil engineer. [inferred]
- **Freshness caveat** [observed]: widening pulls in *evergreen* postings
  (Canonical listings from 2020–2022, Railway's from 2023–24). Narrow
  already marks "Posted N days ago; it may be filled", but they still
  rank as strong fits.

### 8.2 Company data: "does it know enough about the company?"

| | Size | Funding/stage | Investors | Growth | Culture / editorial | Team shape / ownership |
| --- | --- | --- | --- | --- | --- | --- |
| Narrow | from the posting's words | from the posting's words | — | — | — | from the posting's words |
| Simplify | band | stage, total, rounds | notable | headcount growth 6m/1y/2y | "Simplify's Take", letter grades, reviews | — |
| WTTJ | yes | Dealroom rounds | yes | 12-month growth | "Our take", endorsements, Glassdoor | — |
| Wellfound | band | stage badge, rounds | "Top investors" | "Growing fast" | free text | — |
| YC | team size | batch | — | — | founder bios | — |
| HiringCafe | employee count | latest round | yes (filterable) | — | — | — |

What the samples show about Narrow reading companies from postings
[observed]:

- **Stage from boilerplate.** Supabase's "Over $1B raised (including our
  $500M Series F)" in the benefits text is the company reason on every
  Supabase strong fit.
- **"Startup" from award names or prose.** "Named one of America's
  Greatest Startup Workplaces" (Oyster); "a fast-paced startup
  environment" (ClickHouse).
- **Size stated but unread.** Canonical's "1200+ colleagues in 75+
  countries".
- **Unknown stays unknown.** Anthropic's, Stripe's and Airbnb's size is
  never stated, so company shape isn't evaluated.

Conclusion:

- Company metadata would fix *company-shape* errors cheaply. Size and
  latest round are commodity data, and every competitor has them.
  [inferred]
- Of the 24 strong fits in runs A and B that weren't strict strong yeses,
  company shape was the main problem in about 3. Role shape or depth
  caused about 13, eligibility 6, seniority 1, and staleness/domain 1.
  [observed, one judge]
- No competitor knows team size, ownership or engineering culture
  either, so there is no data source to copy for Narrow's most
  distinctive taste dimensions.
- **Company metadata is a real but secondary limitation.** Postings
  alone are insufficient for company *stage/size*, and sufficient only
  sometimes for team and ownership. [inferred]

---

## 9. Remote and international quality

This is where Narrow's thesis is strongest.

| | How remote scope is modeled | Brazil-based remote-only candidate |
| --- | --- | --- |
| Simplify | supported regions; "Remote in [Country]" usually requires living there; remote-anywhere unsupported "due to historical challenges" | **Not supported**; staff "still working on adding support for Brazil" (Jun 2025). ≥10 requests for more regions [community] |
| WTTJ | fixed remote regions: UK, US, CA, FR, DE, IE, NL, ES, "anywhere else within the EU"; no worldwide/LATAM; no timezone | Only by picking US/EU remote, which pulls in invalid roles [observed code] |
| Wellfound | per-job "Hires remotely in", preferred timezones, collaboration hours; **search filter uses company HQ** | Manual checking; "limited for hiring in Latam" [community, anecdote] |
| YC WaaS | US-framed visa values; no timezone | About 3–15% of remote SWE listings usable [observed sample of 38] |
| HiringCafe | structured `workplace_types` plus `flexible_regions` (continent/world); LLM extraction | **Good**: top-15 all valid with the right filters [observed]. Misses: state restrictions hidden in text (175-upvote thread), "Similar jobs" ignores location [community/observed] |
| Jobright, Sonara, HiringCafe agent | US-only | none |
| **Narrow** | deterministic eligibility engine: country, region, authorization, relocation, timezone, contractor; "unknown stays unknown" | **Best of those studied**, with gaps (below) |

Market reality [observed]: in the HN "Who is hiring?" thread for Sep 2026,
137 of 254 posts mention remote:

- about 59 limit it to the US or North America;
- about 50 don't say where;
- about 15 are global, Americas or LATAM.

Three independent Brazilian developers built their own eligibility
filters for exactly this problem (nagringa.dev, and TabNews posts from
around 2025 and around Aug 2026: "Remoto? Sim. Mas só se você tiver work
authorization nos EUA"). [community, repeated]

Narrow eligibility gaps found in this run [observed]:

- "US-Remote, Chicago, Seattle, San Francisco" (locations `[US]`) → read
  as "Remote with no geographic scope". A Stripe strong fit reached
  Today.
- "Remote (United States)" → unclear (Railway Datacenters).
- An EOR company's EU country list (Spain, Romania, Portugal…) → "hires
  employer of records from anywhere" (Oyster GTM).
- US-office-only Temporal roles → "uncertain" rather than a conflict, and
  still strong fits.

Because a strong fit with unclear eligibility may appear on Today (by
design since BRU-322), **5 of 19 run-B strong fits were practically
impossible**. Eligibility is not too strict; for this audience it is
**not strict enough about US-only phrasings**.

---

## 10. Compensation

| | Hard filter | Ranking input | Display | Missing salary | User floor | Location-specific |
| --- | --- | --- | --- | --- | --- | --- |
| Simplify | "dealbreaker" per the extension listing; unclear in Matches | yes | "No salary listed" | not evidently excluded | yes | currency confusion; selector "Planned" [community] |
| WTTJ | unclear | "recommend the roles that fit" | yes | "Jobs with salaries" theme implies not excluded | yes | currency by location |
| Wellfound | search filter | not documented | yes, plus "Wellfound Est." estimates | estimated | yes | currency |
| YC WaaS | unknown | unknown | 36 of 38 remote SWE show salary | not mandatory | asked only if salary matters | US |
| HiringCafe | filter; "hide jobs that don't disclose" optional | no | yes | included by default | filter | currency and period |
| Jobright | match factor | yes | yes | undocumented | range | — |
| Narrow v6 | only a *required*, verified floor | **no** (practicality only) | yes | "unknown, not low" | yes | foreign ranges not compared |

- **No competitor excludes missing pay by default.**
- Salary floors are universal but mostly used as filters, not as reasons
  a job fits.
- Narrow v5's historical design (pay +1 / +1.5 toward the tier; 4 jobs
  surfaced only because of pay) was an outlier. v6's "compensation as
  context, not fit" matches the market and should stay. [documented/observed]
- YC WaaS asking for a floor **only if salary matters** is a good pattern
  for keeping onboarding short. [observed code]

---

## 11. Feedback loops

| | Signals | Friction | Visible improvement | Explains what it learned |
| --- | --- | --- | --- | --- |
| Narrow | Save, Not for me (+ one-click reasons), Not now, Applied, like/dislike | low | not measured | **yes**: "Learned over time", with evidence |
| Simplify | Apply, Save, Hide role/company, thumbs | very low | claimed only | no ("Why this job is a match" explains matches, not learning) |
| WTTJ | Save/Skip (every card), Hide company, "Take another look" | very low (the deck forces a decision) | anecdotal both ways | **partly**: "You've skipped lots of jobs at X companies / using Y. Hide these?" |
| Wellfound, YC | save/apply | — | none | no |
| HiringCafe | hide company, saved searches, Taste Match beta (US) | low | beta | no |
| Sonara | thumbs-down | low | users say no ("keep being presented with jobs I have already thumbs-downed") | no |

**Is Narrow trying to learn too much from too little?** Yes, but it's not
the bottleneck. [inferred]

- Today shows 0–4 items a day, so it collects a handful of signals a
  week. WTTJ's deck collects one per card viewed.
- Narrow's learning is careful: learned taste is weak and never creates a
  strong fit alone. So it cannot rescue an empty or wrong Today.
- WTTJ's "you skipped X — hide these?" is the one pattern with visible,
  user-confirmed learning. Narrow's reason chips are equivalent, but they
  need volume to learn from.

---

## 12. Escape hatches when recommendations are wrong

| | Browse all | Search | Adjust role | Widen location | Browse companies | Out-of-preference pool |
| --- | --- | --- | --- | --- | --- | --- |
| Simplify | yes | yes | preference link on each match | yes | yes | "Discovery picks", "Take another look" |
| WTTJ | no | no (app) | yes | yes | Companies tab | **yes, automatic when the deck runs out** |
| Wellfound / YC | yes | yes | yes | yes | yes | — |
| HiringCafe | yes | **yes** | query | yes | include/exclude | — |
| **Narrow web** | **no** | **no** | via Preferences | via constraints | no | **no**: "Nothing worth your time yet" |
| Narrow CLI/MCP | `find --all` | `find WORDS` | yes | yes | — | "worth reviewing" is listed |

- Narrow already computes a plausible tier ("worth reviewing": 26 in run
  A, 32 in run A2 with an empty Today) and shows it in the CLI and MCP.
  Web users never see it. [observed]
- Exposing it would not turn Narrow into a job board. The pool is a few
  dozen jobs, already filtered by eligibility and fit.
- Evidence it would matter:
  - WTTJ and Simplify both built an out-of-preference fallback.
  - HiringCafe users compensate for no personalization with search
    craft, polling 3 times a day ("apply only to jobs posted in the last
    24h"; a 520-upvote post, 2026-01). [community, repeated]
- Evidence it might not: nobody praises a plausible list for its own
  sake. The value is recovery, and trust that Narrow looked. [inferred]

---

## 13. Trust and wording

- Narrow's claim is the strongest of any product studied: a verdict about
  the whole market ("N of the 7,500 open jobs Narrow checked are worth
  your attention today").
- That claim was 2 of 4 strict on the production boards and 0 of 4 on
  the expanded boards. [observed]
- The claim is defensible only if precision on *real* postings is high.
  Today it's measured only on fixtures.
- A softer promise alone won't fix quality. But the distinction the brief
  proposes is supported:
  - **Today**: strong fits, only when strict precision on real postings
    is measured as high.
  - **Also plausible**: the worth-reviewing tier, labeled as such.
- Both WTTJ ("matches outside of your preferences") and Simplify
  ("Discovery picks outside your usual matches") use exactly this split.
  [observed/documented]

---

## 14. Narrow strengths (tested against evidence, not assumed)

| Strength | Evidence it's real | Evidence users care |
| --- | --- | --- |
| **Deterministic eligibility for non-US remote** (country, LATAM, timezone, authorization, contractor) | No competitor handles Brazil well; HiringCafe comes closest [observed] | **Strong**: ≥3 Brazilian builders made their own filters; "remote means US-only" is a repeated theme [community] |
| **First-party ATS sources and verification** ("verified open just now") | Same approach as HiringCafe/Simplify [documented] | **Strong**: three HN front-page ghost-job threads (2025–26, 139–253 comments); "no reposts/agencies" is HiringCafe's top praise [community] |
| **Transparent reasons with quotes** ("why", "things to consider") | Richer than Simplify's panel; WTTJ's "why a match" is still "later in 2026" | **Moderate**: one "no way to see why" complaint about Jobright; explanations only help when the pick is right [community, anecdote] |
| **Fit/practicality separation; unknown pay stays unknown** | Matches market practice (§10) | Low salience in community talk |
| **Specialization depth, IC vs management** | Unique; HiringCafe's IC/manager tags were wrong for 7 of 20 in a user audit | Plausibly valuable, but the rules missed K8s internals, ZK and non-engineering roles in this run |
| **OSS / local-first / user-owned data** | Real | **Weak**: open-source job tools on Show HN got 1–8 points [observed]. A credibility asset, not a reason people come |
| **No mass apply** | Real | **Split**: some users run auto-submit funnels that "work upsettingly well" [community, 3, divergent] |

---

## 15. Narrow weaknesses

1. The **role is inferred, not asked**. With no work shape in the words,
   Today is empty. [observed]
2. **The universe** is 17 mostly large, US-centric companies, with 94% of
   jobs out of reach for the target user. [observed]
3. **Reading real postings.** Description-inferred work shapes promote
   non-engineering roles: curriculum developer, GTM engineer, solutions
   architect, customer success, support. Depth rules miss specialties
   outside their list: K8s internals quoted as a positive, ZK proofs.
   [observed]
4. **Benchmark overfit.** 100% on 27 fixture jobs against about 25% strict
   on 32 real strong fits. [observed]
5. **US-only phrasings** read as unclear, and unclear-eligibility strong
   fits allowed on Today. [observed]
6. **No escape hatch in the web app.** [observed]
7. **Company facts** come from posting prose (boilerplate, awards).
   [observed]
8. **Free-text taste reader errors.** "ML model-training roles" became
   "avoid AI". [observed]
9. **Evergreen postings** from 2020–2024 rank as strong fits. [observed]

---

## 16. Root-cause diagnosis

Ranked by expected impact on "Today feels worse than competitors".

| Rank | Cause | Evidence | Confidence | Expected impact |
| --- | --- | --- | --- | --- |
| 1 | **B. Missing explicit target role** (with A/F: intent capture and the reading of the words) | 13 → 0 strong fits on identical data [observed]; every competitor requires a role pick [documented/observed] | **High** | **High**: the difference between an empty and a non-empty Today |
| 2 | **E. Job source coverage (aim)** | 94% of the universe unreachable; practical strong yeses at 2 companies; remote-first boards triple eligible yield; HiringCafe's top 20 holds 6 right companies Narrow never reads [observed] | **High** | **High**: caps Today at a few companies whatever the ranker does |
| 3 | **I/F. Reading real postings (work shape, depth)**: the rules model, not the threshold | Strong-tier strict precision 23–26% on live boards against 100% on fixtures; top-ranked FPs are non-engineering roles [observed] | High (direction), medium (magnitude, one judge) | **High, and rising with coverage**: widening sources without this makes Today worse (run B) |
| 4 | **G (inverted). Eligibility not strict enough on US-only phrasings** | "US-Remote", "Remote (United States)", EOR lists, US offices → unclear, reaching the strong tier and Today [observed] | High | Medium: trust-destroying for this audience, cheap to fix |
| 5 | **J + K. The promise is too strong, and there's no escape hatch in the web app** | Strongest wording of any product; competitors all offer out-of-preference pools or search [observed/documented] | Medium | Medium: affects trust and recovery, not precision |
| 6 | **D. Company metadata** | Stage from boilerplate, size unread; competitors all attach company data; about 3 of the 24 non-strict strong fits [observed] | Medium | Low–medium now; more once role and eligibility are fixed |
| 7 | **C. Profile/taste inference** | The AI-avoid misreading; the profile's role inference is soft by design [observed] | Medium | Low if #1 is done (a firm pick replaces the inference) |
| 8 | **L. Other: evergreen/stale postings** | 2020–2024 postings as strong fits [observed] | Medium | Low–medium, rising with coverage |
| 9 | **H. Today threshold too strict** | When intent is stated, strong fits exist and many are really Maybes. The threshold is, if anything, too *loose* on real postings [observed] | Medium | **Low**: loosening would make things worse |
| 10 | **Feedback learning** | Low volume by design; learned taste can't create strong fits [documented] | Medium | Low now |
| — | **Competitors are not actually better** at ranking | No comparable logged-in samples; HiringCafe ≈ Narrow on strict precision; community reports ignored preferences and noisy tags [observed/community] | Medium | Explains why copying their matching wouldn't help; their edge is breadth, the anchor, context and framing |

---

## 17. Small experiments (at most three)

None needs a ranking-rule rewrite. Each is measured on **real postings
judged by the person (or two judges)**, not on fixtures.

### Experiment 1: one explicit role question

- **Change:**
  - In onboarding and Preferences, ask "What kind of role are you looking
    for?" as a multi-select from the existing `work_shape` taxonomy
    (backend, platform, infrastructure, product, full stack, …), plus an
    optional free-text title.
  - Store picks as *stated, confirmed* `work_shape` statements (firm).
  - Keep the free-text description as "Anything else you care about?"
  - No ranking change.
- **Measure** on the dogfood account and 2–3 synthetic personas, one
  snapshot:
  - the share of profiles with ≥1 strong fit;
  - Today size;
  - strict precision of Today on judged items;
  - time to complete onboarding.
- **Falsified if:** Today stays empty for the dogfood user with a role
  picked, **or** strict precision of the resulting strong tier on judged
  real postings is below 50%. In that case the bottleneck is reading and
  coverage, not intent.
- **Reversible:** it's one stored statement per pick.
- **Relation to the pending work-shape fallback:**
  - This measures an explicit pick against the planned profile fallback.
  - The run-A2 evidence says a profile inference is *soft* and only
    reaches "worth reviewing".
  - So the fallback alone is unlikely to fill Today unless it's made
    firm, which is exactly what asking avoids.

### Experiment 2: widen the universe on a judged real-posting set, with the existing semantic reviewer as the precision guard

- **Change (config only):** in a throwaway or staging environment:
  - add the 43 remote-first boards (Appendix A), or a curated list of
    companies known to hire in Brazil/LATAM, to the 17;
  - rank the persona and the dogfood profile with the semantic reviewer
    **off and on** (`review_fit`; already built, off by default).
- **Measure:** a 2 × 2 (17 vs 60 boards × reviewer off/on). For each
  cell, the person judges every strong fit and the top 10 plausible:
  - strict and relaxed precision, obvious-FP rate;
  - practical strong yeses, and distinct companies among them;
  - Today strict precision.
- This also becomes the seed of an **out-of-sample benchmark set**: real
  postings, frozen. This run's 32 judged strong fits are a start.
- **Falsified if:**
  - widening doesn't at least double practical strong-yes *companies*
    (this run: 2 → 5 across A+B); or
  - the reviewer doesn't cut obvious FPs in the wide universe by half
    (curriculum developer, GTM, ZK, K8s internals). If it doesn't, the
    reading problem needs more than the existing reviewer. Decide that
    before any rules work.
- **Reversible:** config.
- **Cost:** about 30 reviews, roughly $0.60 once, per the estimate in
  fit-and-practicality.md.

### Experiment 3: show the plausible tier in the web app without weakening Today

- **Change:**
  - Below Today (or when Today is empty), a collapsed "Also plausible"
    list of the existing worth-reviewing tier, eligibility-checked,
    labeled as not a recommendation.
  - Save/Not-for-me work on it the same way.
  - When Today is empty, say what was checked and why nothing qualified:
    for example, "2,900 checked · 2,242 on-site or relocation · 481 not
    open to Brazil · 177 you can take · 0 strong fits: tell us the kind
    of role you want".
  - No ranking change.
- **Measure** over 2–4 weeks of dogfood:
  - saves and applies from "Also plausible" against Today;
  - jobs saved there that Today never showed;
  - return visits after an empty Today.
- **Falsified if** the person saves nothing from the plausible list in 2–4
  weeks. Then the plausible tier isn't useful, and more surface won't
  help.
- **Reversible:** a UI flag.

Order: 1 and 2 are independent and can run in parallel. Run 3 after 1,
because otherwise it mostly displays the consequence of missing intent.

---

## 18. Product-thesis assessment

The current promise: "Narrow watches thousands of jobs and shows only the
few worth your time."

- **Differentiated?** Yes, for this audience. Nobody else combines:
  - first-party verification;
  - deterministic non-US remote eligibility;
  - fit reasons with quotes;
  - no quota filling.

  The Brazil/LATAM remote gap is unaddressed by Simplify, WTTJ,
  Jobright, Sonara and HiringCafe's agent, and only partly by Wellfound
  and YC. Users build their own filters. [observed/community]
- **Achievable?** Not yet, as measured:
  - the universe caps it;
  - intent capture can leave it empty;
  - on real postings its "strong fit" is about 25% strict.

  So it is **differentiated but technically brittle today**.
- **Too brittle to keep?** No. The three causes are concrete and cheaply
  testable: the role pick, the source aim, the reviewer/reading guard.
  None requires abandoning the idea.

Alternative phrasings, tied to behavior:

| Promise | Honest today? | Condition |
| --- | --- | --- |
| "…shows only the few worth your time" (current) | **No**: 0 of 4 strict on the expanded boards | only after Experiment 2 shows ≥70% strict on real postings |
| "Narrow watches the market you can actually take, and ranks what fits you" | **Yes**: eligibility is the proven strength; ranking is honest about being a ranking | pairs with Experiment 3 |
| "Stop searching dozens of job boards. Narrow watches them and helps you decide what deserves attention" | Only after widening | needs Experiment 2's universe |
| "Narrow learns the companies and roles you want, then watches for them" | Weakly: learning volume is low | not evidenced |

---

## 19. Recommended next action

### Verdict: **continue with a narrower change**

Continue the thesis, with these changes:

- Ask for the target role explicitly (Experiment 1).
- Aim the universe at companies that hire remotely in Brazil/LATAM, and
  judge precision on real postings with the existing reviewer as a guard
  (Experiment 2).
- Tighten the US-only phrasings.
- Soften Today's wording until real-posting precision is measured, and
  give the web app the plausible tier as an escape hatch (Experiment 3).

**Not** a pivot. The evidence says the differentiation is real (non-US
remote eligibility, verification) and the failures are in inputs and
reading, not the idea.

**Not** "continue the current thesis unchanged". The benchmark's 100% is
not what users experience.

### What not to build next

- **Another large rules pass over the ranker** (a BRU-322-sized rewrite)
  before Experiment 2 says whether the semantic reviewer fixes reading.
  More hand-written rules would again pass the fixtures and miss the
  next unfamiliar posting.
- **A loosened Today threshold** or quota filling. On real postings the
  threshold is already too loose.
- **A general job board or search engine.** HiringCafe does breadth
  better and free. Expose Narrow's own plausible tier, not the market.
- **Feedback-learning upgrades.** There's too little signal at 0–4 items
  a day for learning to matter yet.
- **Auto-apply or agentic application.** Community evidence says it
  optimizes volume, not precision (Sonara, LazyApply, Jobright); it's
  the opposite of the thesis.
- **A company-metadata integration** before role and eligibility are
  fixed. It helps, but it's third in line.

### Proposed tasks (for review; not created in Linear)

1. **Explicit role pick in onboarding and Preferences** (Experiment 1),
   with a before/after on the dogfood profile.
2. **Out-of-sample benchmark and wider-universe trial** (Experiment 2):
   freeze a real-posting snapshot from about 60 boards, judged by Bruno;
   run the 2 × 2 with the semantic reviewer.
3. **Eligibility: read US-only phrasings as a scope.** "US-Remote", "Remote
   (United States)", US-only office lists, EOR country lists. These are
   small, deterministic, rule-matrix cases.
4. **Web "Also plausible" plus an honest empty state** (Experiment 3),
   behind a flag.
5. Later, conditional on 1–2: company size and stage from a data
   source, for shortlisted companies only.

---

## Appendix A: run B board set (chosen before reading postings)

- **Ashby:** airbyte, attio, axiom, clerk, clickhouse, convex-dev,
  duck-duck-go, knock, mintlify, n8n, neon, oyster, paddle, plain,
  railway, render, resend, sanity, sentry, socket, temporal, warp, workos,
  zapier, inngest.
- **Greenhouse:** automatticcareers, chainguard, circleci, cockroachlabs,
  gitlab, grafanalabs, honeycomb, launchdarkly, planetscale, remotecom,
  sourcegraph91, tailscale, vercel, wikimedia, canonical, ebanx,
  wildlifestudios.
- **Lever:** pipedrive.

Chosen for reputation as remote-first or international employers
(startup to growth stage, plus two remote-first larger ones: GitLab and
Canonical), and probed only for board existence on 2026-09-30. Not
validated as a production source list.

## Appendix B: how to reproduce the Narrow runs

```bash
cargo build --release -p jobhunt-cli
# per run directory (A: the [[sources]] of deploy/cloud.toml; B: Appendix A),
# config.toml with [storage] database = "jobhunt.db" and the sources
N=target/release/narrow; C="--config $DIR/config.toml --database $DIR/jobhunt.db"
$N $C init persona-resume.md          # the benchmark's senior-startup-generalist resume
$N $C preferences describe "Senior backend, platform or product-infrastructure engineering as an individual contributor. Small technical teams with high ownership and broad responsibility, at startups or growth-stage, engineering-driven companies. I don't want early-career roles, engineering management, research or ML model-training roles, deep database-engine internals, or large process-heavy companies."
$N $C preferences confirm
$N $C preferences remove <the "Ai" avoid statement>   # the reader's misreading (§4)
$N $C preferences set location "São Paulo, Brazil"
$N $C preferences set work-setup remote-only
$N $C preferences set relocation no
$N $C preferences set authorized-in Brazil
$N $C find --refresh -n 20
$N $C find --offline --json -n 25 [WORDS]
# A2: copy A's database, remove the backend/platform/infrastructure statements, find --offline
```

Live boards change daily; the job counts above are from 2026-09-30.

## Sources

Dates are publication dates where known, otherwise "accessed 2026-09-30".

**Simplify**
- help.simplify.jobs: [Using your Job Matches](https://help.simplify.jobs/articles/2166608-using-your-job-matches), [Setting match preferences](https://help.simplify.jobs/articles/7272801-setting-your-job-match-preferences), [Matches vs Job Board](https://help.simplify.jobs/articles/6542018-job-matches-vs-job-board), [Signing up](https://help.simplify.jobs/articles/0738509-signing-up-for-simplify), [Supported countries and regions](https://help.simplify.jobs/articles/6895855-supported-countries-and-regions), [Popular Match Groups](https://help.simplify.jobs/articles/1906057-using-popular-match-groups), [Keywords score](https://help.simplify.jobs/articles/2175778-understanding-the-keywords-score) (all updated about Jun–Aug 2026)
- simplify.jobs /ai-talent-agent, /llms.txt, /c/Vercel, /l/List-Remote-Software-Engineering-Jobs-2026, the /preferences quiz JS bundle (accessed 2026-09-30)
- simplifyjobs.featurebase.app feedback posts: remote-global-filter, there-is-brazil, you-sure-you-take-preferences-into-account, filter-out-job-between-ic-and-manager, tpm-job-family-to-be-added, locations (Mar 2025–Sep 2026)
- [Trustpilot: simplify.jobs](https://www.trustpilot.com/review/simplify.jobs) (reviews May 2025–Sep 2026); [Chrome Web Store: Simplify Copilot](https://chromewebstore.google.com/detail/simplify-copilot-autofill/pbanhockgagggenencehbnadejlgchfc) (updated 2026-09-30)

**Welcome to the Jungle / Otta**
- App bundle static.otta.com/frontends/search-wttj (last-modified 2026-09-15); app.welcometothejungle.com /preferences and company pages (accessed 2026-09-30)
- [TechCrunch: Otta](https://techcrunch.com/2019/12/04/otta/) (2019-12-04); [Fortune](https://fortune.com/2022/02/07/this-job-search-site-is-trying-to-be-the-solution-to-the-great-resignation-by-prioritizing-picky-candidates/) (2022-02-07); [Otta acquired by WTTJ](https://press.welcometothejungle.com/en/news/uk-recruitment-platform-otta-acquired-by-welcome-to-the-jungle) (2024-01-22); [WTTJ on salary transparency](https://solutions.welcometothejungle.com/en/blog/why-candidates-need-salary-transparency) (2026-03-19)
- help.welcometothejungle.com: "how does the matching algorithm work", "what is job matching" (©2025, accessed 2026-09-30)
- [App Store: Welcome to the Jungle (Otta)](https://apps.apple.com/us/app/welcome-to-the-jungle-otta/id1640509194) (versions 2023–2026); [Trustpilot: otta.com](https://www.trustpilot.com/review/otta.com) (reviews 2025–2026); [HN 39683061](https://news.ycombinator.com/item?id=39683061) (2024-03-12)
- Reddit via Pullpush: r/jobhunting 1mbsgd3 (2025-07-28), r/cscareerquestionsuk 1wabke4 (2026-09-08), r/AskChicago 1wdlz9a (2026-09-21), r/ExperiencedDevs 1rmojk3 (2026-03-13), r/cscareerquestionsuk 1t4gygo (2026-05-05)

**Wellfound**
- help.wellfound.com articles 773, 1038, 758, 782, 774, 767, 786, 1019 (updated 2023-02/03); [762: remote job search filters](https://help.wellfound.com/article/762-remote-job-search-filters) (2025-12-08); wellfound.com/candidates/remote (accessed 2026-09-30); [Mux: Wellfound](https://mux.com/blog/wellfound) (2026-04-14)
- Wayback snapshots of company and job pages (2026-07 to 2026-09)
- [Trustpilot: wellfound.com](https://www.trustpilot.com/review/wellfound.com) (reviews Jun 2025–Sep 2026); HN [47617555](https://news.ycombinator.com/item?id=47617555) (2026-04-02), [48848675](https://news.ycombinator.com/item?id=48848675) (2026-07-09)

**YC Work at a Startup**
- [ycombinator.com/companies/posthog](https://www.ycombinator.com/companies/posthog), [/jobs/role/software-engineer/remote](https://www.ycombinator.com/jobs/role/software-engineer/remote), [workatastartup.com/faq](https://www.workatastartup.com/faq), the WaaS ProfileWizard JS bundle (accessed 2026-09-30)
- HN [46764091](https://news.ycombinator.com/item?id=46764091) (2026-01-26), [42857497](https://news.ycombinator.com/item?id=42857497) (2025-01-28), [45104624](https://news.ycombinator.com/item?id=45104624) (2025-09-02)

**HiringCafe**
- [hiringcafe.com](https://hiringcafe.com/) search, filters and the /ai-search page (observed 2026-09-30); /about (accessed 2026-09-30)
- [Founder on HN](https://news.ycombinator.com/item?id=42806956) (2025-01-23); [Scaling HiringCafe](https://blog.hiring.cafe/p/scaling-hiringcafe-from-0-to-1m-users) (2025-09-17); [Pre-seed press release](https://www.webwire.com/ViewPressRel.asp?aId=361033) (2026-09-28)
- r/hiringcafe: 1vqsyph Taste Match (2026-08-17), 1swsyen agent launch (2026-04-27), 1wj12a9 tag audit (2026-09-17), 1tq3txk (2026-05-28), 1qg8544 (2026-01-18), 1oj7un1 (2025-10-29), 1rfdxh9 (2026-02-26); [HN 48658280](https://news.ycombinator.com/item?id=48658280) (2026-06-24)

**Sonara and auto-apply**
- [sonara.ai](https://www.sonara.ai/), /terms-conditions (Terms 2024-11-05; site © 2026 Bold Limited; accessed 2026-09-30)
- [Trustpilot: sonara.ai](https://www.trustpilot.com/review/sonara.ai) (reviews May–Sep 2026); shutdown history from secondary/competitor sources (LoopCV, Jul 2026), unverified
- [Scoutify: LazyApply review](https://scoutify.com/blog/lazyapply-review) (about Aug 2025, competitor)

**Jobright, Himalayas, Torre**
- [jobright.ai](https://jobright.ai/), [/ai-job-match](https://jobright.ai/ai-job-match) (accessed 2026-09-30); [Trustpilot: jobright.ai](https://www.trustpilot.com/review/jobright.ai) (Jun–Sep 2026); [App Store](https://apps.apple.com/us/app/jobright-ai-job-search/id6738236788) (Feb–Nov 2025); [Hirecarta review](https://hirecarta.com/blog/jobright-review) (2026-04-20, competitor)
- [himalayas.app/jobs/countries/brazil](https://himalayas.app/jobs/countries/brazil) (accessed 2026-09-30); [torre.ai](https://torre.ai/), [Trustpilot: torre.ai](https://www.trustpilot.com/review/torre.ai) (2025–2026)

**Community, cross-product**
- HN: ["What job boards are good these days?"](https://news.ycombinator.com/item?id=49816567) (2026-09-23); ["Job seekers, what's working?"](https://news.ycombinator.com/item?id=46588695) (2026-01-12); ["Is LinkedIn Job Search the worst?"](https://news.ycombinator.com/item?id=42857569) (2025-01-28); ["How would you improve job search?"](https://news.ycombinator.com/item?id=42623386) (2025-01-07); ["How are you using AI to find jobs?"](https://news.ycombinator.com/item?id=45192594) (2025-09-10)
- Ghost-job threads: [45028785](https://news.ycombinator.com/item?id=45028785) (2025-08-26), [46309061](https://news.ycombinator.com/item?id=46309061) (2025-12-18), [48558338](https://news.ycombinator.com/item?id=48558338) (2026-06-16)
- ["Who is hiring?" Sep 2026](https://news.ycombinator.com/item?id=49522897) and ["Who wants to be hired?" Sep 2026](https://news.ycombinator.com/item?id=49522896) (2026-09-01)
- [LinkedIn search changes](https://joeapfelbaum.substack.com/p/linkedin-search-changes-to-ai-based) (Nov 2025)
- Brazil: [TabNews: an aggregator of international jobs for Brazilians](https://www.tabnews.com.br/lucasfaria/como-criei-um-agregador-de-vagas-na-gringa-pra-brasileiros) (about 2025); [TabNews: an agent for international applications](https://www.tabnews.com.br/vinicius91carvalho/cansei-de-passar-noites-aplicando-para-vaga-gringa-entao-construi-um-agente-que-faz-o-ciclo-inteiro-descoberta-formulario-recrutador-e-entrevista) (about Aug 2026); [nagringa.dev](https://www.nagringa.dev/vagas) (accessed 2026-09-30); [vaganagringa.dev](https://vaganagringa.dev/onde-procurar-vagas-remotas/) (Jun 2026)

**Narrow (this repository)**
- [recommendation-benchmark.md](recommendation-benchmark.md), [fit-and-practicality.md](fit-and-practicality.md), [taste-profile.md](taste-profile.md), `deploy/cloud.toml`, `apps/web/src/components/today.test.tsx`
- Live runs A, A2 and B, 2026-09-30 (§7, Appendix B)
