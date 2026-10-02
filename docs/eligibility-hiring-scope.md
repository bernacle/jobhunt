# Eligibility patch: hiring scope (BRU-325)

Run date: **2026-10-02**. A deterministic Practicality fix for the
geography errors isolated in
[`real-posting-precision-experiment.md`](real-posting-precision-experiment.md)
§14. Eligibility `RULES_VERSION` 5 → 6. It changes no Fit reading,
semantic classifier or reviewer, ranking weight, Today threshold, source or
company metadata.

## 1. Rules

The rules are documented in the README ("Remote scope", "Restrictions",
"Hiring-scope precedence and conflicting evidence"). In short:

| Rule | Reads | Example |
| --- | --- | --- |
| Remote scope in a list | a list part pairing a country or region with "Remote" is a **stated** scope; countries listed with it share it; cities stay offices | `US-Remote, Chicago, Seattle, San Francisco` → remote in the US |
| Bare country beside bare "Remote" | the country (or region) is the **stated** scope, not an office | `Remote` + location `US` |
| Finite list | a remote job whose fields list only places, with nothing unscoped beside them, is **listed** in their countries (decisive) | remote + Seattle, Austin, San Francisco → US; remote + London, Manchester → UK |
| Region codes | `NAMER`/`NORAM` is North America; "American region(s)" the Americas; `A & B` two places | Zapier, Canonical, WorkOS |
| Hiring language | "based within", "located within", "available to applicants", "eligible countries", … | "this role requires you to be based within EMEA" |
| Scope labels | a `Location:`/`Countries:`/`Region:` line is this role's scope, clause by clause; "Location: Fully remote" opens a remote option | Zapier, Wikimedia, Canonical, Oyster |
| Pay is not hiring | sentences about pay, salary, compensation, base or annual range, benefits; and "For US-based applicants: …" terms | Wikimedia's US pay range |
| Statement subject | each place statement is about **this role** ("this role", "this position", "a remote position", a scope label) or **hiring in general** | Railway, Linear |
| EOR's own name | at Oyster (or Deel, …), "Oyster" is the company, not an engagement path | Oyster's "Work from anywhere: Oyster has no borders" |

## 2. Precedence

1. explicit hiring statement in the description;
2. stated scope in the location fields;
3. finite list of places in the location fields;
4. workplace type or remote flag alone (scope unknown);
5. unknown.

Pay sentences are never evidence. Conflicts:

- **Narrower description**: applies, whoever it is about (unchanged).
- **Wider description about this role**: applies, with the resolution
  shown. Zapier (`NAMER; APAC; EMEA` + "Location: Americas - North,
  Central and South America, EMEA, APAC"), Railway *Datacenters* (`Remote
  (United States)` + "This is a remote position available anywhere in the
  world! Linkedin makes us show a country"), Linear (`Europe` + "This role
  is open to candidates based in North America and Europe").
- **Wider description about hiring in general**: unknown, as before ("we
  are open to candidates across the Americas" under `Remote (US)`).

## 3. Frozen-corpus measurement

Same frozen corpus (`corpus-2026-10-01.db`, 4,622 open postings), same
candidate (São Paulo, remote only, no relocation; Backend · Platform ·
Infrastructure), same probe, instant and feed call as BRU-325; reviewer
off. "Before" is `main` at `d9cc368`. Labels are BRU-325's 50 frozen
judgments; postings outside them are marked.

### 3.1 Wider sources (B1)

| | Before | After |
| --- | ---: | ---: |
| Open jobs | 4,622 | 4,622 |
| Ineligible · work-setup/relocation conflict | 1,487 · 2,648 | 1,626 · 2,643 |
| **Actionable** | 487 | **353** |
| Strong · plausible · insufficient · poor | 33 · 77 · 54 · 323 | 29 · 67 · 44 · 213 |
| **Impossible in the strong tier** | 6 | **0** |
| Strong tier: Strong yes · Maybe · No | 8 · 14 · 5 | 8 · 14 · 5 (+2 not in BRU-325's set) |
| **Practical Strong yes (jobs · companies)** | 8 · 5 | **8 · 5** |
| Today (companies · jobs) | 5 · 14 | 5 · 12 |
| **Impossible on Today** | 1 | **0** |
| Today: Strong yes · Maybe · No | 3 · 8 · 2 | 2 · 6 · 2 (+2 not in the set) |
| Practical Strong-yes companies on Today | 2 | 1 |
| Top-10 plausible: impossible | 0 | 0 |

Today, after:

| | Job | Judged |
| --- | --- | --- |
| lead | ClickHouse: Senior Curriculum Developer & Instructor | No (fit) |
| lead | Supabase: Platform Engineer - Compute Capacity, + the same 7 peers | 2 Strong yes, 5 Maybe, 1 No |
| lead | **Wikimedia**: Senior Software Engineer, Core Experiences (Contract) | not in the set; practically valid (Brazil listed). On fit, a No: a 3-month contract, full-stack Vue/PHP mobile web (post-hoc, not blind) |
| lead | Sourcegraph: Software Engineer - Platform [IC3] | Maybe |
| lead | **Zapier**: Staff Backend Engineer, Commerce | not in the set; practically valid (South America). On fit, a Maybe: backend, but staff-level billing-domain depth in a mature monolith (post-hoc) |

Left Today: Oyster *Senior GTM Engineer* (Impossible, EMEA only) and its
peer *Senior Engineer (Platform)*, plus Canonical *Backend SaaS* and
*MAAS* (Maybe), displaced by Wikimedia and Zapier. *Senior Engineer
(Platform)* is still a strong fit. Its eligibility moved from
eligible to **uncertain**: the old "eligible" came only from Oyster's own
"Work from anywhere: Oyster has no borders" read as an EOR path. Its
fields list Chile, Colombia, Peru and Argentina but not Brazil. The
description says "all of Oyster's positions are fully remote" and gives a
time-zone window. So its gate is now EligibilityUnclear, and it ranks
below the newly eligible leads. That is why practical Strong-yes companies
on Today go 2 → 1. The ranking's ordering is unchanged.

### 3.2 Current sources (A1)

| | Before | After |
| --- | ---: | ---: |
| Actionable | 175 | 93 |
| Strong | 13 | 12 |
| Impossible in strong tier · on Today | 1 · 1 | 0 · 0 |
| Practical Strong yes (jobs · companies) | 3 · 2 | 3 · 2 |
| Today (companies · jobs) | 4 · 13 | 3 · 12 |

Stripe *Backend Engineer, Core Technology* (`US-Remote, …`) leaves Today;
nothing replaces it.

### 3.3 Every decision that changed (B1, 4,622 postings)

| Change | Jobs | What |
| --- | ---: | --- |
| uncertain → ineligible | 137 | |
| eligible → ineligible | 12 | Oyster EMEA/NL roles (EOR misread), Canonical roles with a narrower "Location:" line |
| ineligible → eligible | 14 | Wikimedia 11 (pay sentence), Zapier *Commerce*, Canonical *Web Frontend* (pay sentence), Canonical *Observability* ("EMEA and Americas regions") |
| uncertain → eligible | 1 | Zapier *GTM Analytics* ("Location: Americas") |
| eligible → uncertain | 5 | Oyster ×4, GitLab *SRE (UK)*: eligible before only through a misread; now an honest conflict |
| ineligible → uncertain | 1 | Wikimedia *Executive Communications*: the pay sentence no longer rules it out; a time-zone reading leaves it unclear |

New exclusions (149) by rule: finite list 75 (Spotify 25, Temporal 18,
n8n 10, Airbnb 8, …); stated scope 58 (Stripe 43, Zapier `NAMER` 5, …);
description scope 14 (Canonical "Location:" lines, Automattic, Supabase
APAC, Oyster EMEA); office-only Oyster postings 2. Each of the 55 whose
descriptions mention Brazil, LATAM or worldwide words was read: none is a
false exclusion; there, those words describe customers, markets or the
company.

### 3.4 Named postings

| Posting | Before | After |
| --- | --- | --- |
| Stripe *Backend Engineer, Core Technology* (`US-Remote, Chicago, Seattle, San Francisco`) | uncertain, strong, A1 Today lead | **ineligible**: "limits remote work to the United States" |
| Stripe *Integration Engineer (Metronome)* ×2 (`Remote` + `US`) | uncertain | **ineligible** |
| Temporal *Cloud Platform Foundations*, *SWE II*, *Traffic* (Seattle/Austin/SF) | uncertain, strong | **ineligible**: "lists only places in the United States" |
| Temporal *Senior Platform Architect* (London, Manchester) | uncertain, strong | **ineligible** (UK) |
| Oyster *Senior GTM Engineer* ("based within EMEA") | eligible, strong, B1 Today lead | **ineligible** |
| Wikimedia ×12 ("Countries: Brazil, …"; US pay range) | ineligible | 11 **eligible**, 1 uncertain (time zone) |
| Zapier *Staff Backend Engineer, Commerce* | ineligible | **eligible**, strong, Today lead |
| Railway *Infra Engineer - Datacenters* | uncertain (conflict) | geography resolved (role statement); still uncertain on an unrelated time-zone reading |

## 4. New errors and remaining gaps

- **New false inclusion: none found.** Two Oyster postings that were
  eligible only through the EOR misread (*Content Marketing Lead*, *Senior
  AI Solutions Engineer - GTM*) stay eligible, now from their own
  "Location: Fully remote" and "AMER or EMEA" statements.
- **New false exclusion: none found** after the audit. The first run
  found five, all fixed before measuring: "any American region" read as
  the US; a "strongly preferred" clause leaking into "remote (US)";
  `United States & Canada` read as the US; two Oyster postings saying
  "Location: Fully remote".
- **Remaining gaps:**
  - Oyster's "Work from anywhere: Oyster has no borders" perk is still
    "anywhere" (general) when nothing else scopes a role. *Business
    Manager, GTM* stays eligible on it.
  - A pointer to another posting ("Candidates based in the US or Canada
    can apply to this posting: … AMER") is read as allowing those places
    (GitLab *SRE (UK)*: uncertain, should be ineligible).
  - Anthropic's "Remote-Friendly (Travel-Required) | SF | Seattle | NYC"
    and Axiom's `New York; Remote` stay unscoped. A city beside an
    unscoped "Remote" only suggests a country, by design.
  - A finite list assumes the listed places are the scope. A global
    company listing only its HQ city with no worldwide statement would
    now read as that country. None was found in this corpus.

## Reproduce

```bash
cargo build --release -p jobhunt-cli --example eligibility_probe --example real_posting_probe
# on copies of BRU-325's B1 (and A1) databases, once per binary (before: d9cc368)
target/release/examples/eligibility_probe B1/config.toml B1/jobhunt.db > decisions.jsonl
target/release/examples/real_posting_probe B1/config.toml B1/jobhunt.db 2026-10-01T20:05:00Z > cell.json
```
