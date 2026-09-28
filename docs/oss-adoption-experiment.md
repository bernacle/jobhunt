# OSS adoption experiment

Tracked as BRU-315 ("Validate OSS-first adoption and Narrow Cloud value").
This document defines a disciplined experiment for a small cohort of real
external users. It is not a roadmap and does not commit to building a
hosted Narrow Cloud product.

**Status: experiment ready; awaiting real-user evidence.** Nothing below
should be read as a decision to build Cloud, add billing, or change what
Narrow does. The only job right now is to observe.

## Core hypothesis

External users can install and use Narrow (the OSS product, run on their
own machine) and receive a shortlist that feels more valuable than
manually browsing job boards.

## Secondary hypothesis

Repeated, independent, operational friction — not one-off setup
complaints — may reveal a reason a hosted Narrow Cloud would be worth
building. Feature requests alone are not evidence; see
[Cloud-signal taxonomy](#cloud-signal-taxonomy) and the
[decision framework](#oss-vs-cloud-decision-framework).

## Why this is needed now

Narrow is public, Apache-2.0, anonymously cloneable, and its CI is green.
The product thesis ("Narrow searches the job market for you and surfaces
only the opportunities worth your attention") has never been tested
against a person who is not the maintainer. This experiment is that test.

## 1. Clean-room outsider evaluation (done once, as a usability check only)

A clean-room pass was run in an isolated environment against an
anonymous, unauthenticated clone of the public repo
(`https://github.com/bernacle/jobhunt`), using only what is documented in
`README.md`, `CONTRIBUTING.md`, `docs/cloud.md`, `config.example.toml`,
and `--help` output — no maintainer knowledge.

**This is agent-driven usability testing, not product-market-fit
evidence.** It tells us whether the golden path *works*; it cannot tell
us whether a human *wants* it. Real external users are still required
before BRU-315 can progress (see [What requires real
users](#what-requires-real-users-before-bru-315-can-progress)).

### Result: the golden path works

From a fresh clone, with zero source configuration: `cargo build
--release` (~7 minutes, no errors), `jobhunt init <resume>`, `jobhunt
preferences add "..."`, `jobhunt find` — checked ~2,895 live jobs across
17 built-in company sources in under 5 seconds, returned a 5-item
shortlist with legible, traceable reasoning (`jobhunt why`, `jobhunt
show`). `jobhunt save` / `jobhunt like` worked, including documented
idempotency ("Already saved ...: nothing changed."). A second `jobhunt
find` made no network request and reflected the new feedback in ranking,
exactly as documented. Time to a real, reasoned shortlist from a blank
profile: well under a minute of CLI interaction (excluding the one-time
build). The shortlist genuinely stood in for browsing thousands of raw
postings — it did not degrade into an inventory dump.

### Issues found, classified

| Classification | Area | Issue |
| --- | --- | --- |
| Confusing | Config loading | `--config` / `JOBHUNT_CONFIG` pointed at a **not-yet-existing** file hard-errors on every command ("No such file or directory"), while the default config path — also absent on a fresh install — silently falls back to defaults. This contradicts the README's lead claim that "everything has a default, so no config file is needed," specifically for the one workflow (a custom config location) the README itself suggests. |
| Nice-to-have | Sources / docs | The 17 built-in default sources (the companies actually searched with zero configuration) are never enumerated in the README; only discoverable by running `jobhunt config` or `jobhunt doctor` after building. |
| Nice-to-have | Sources / docs | `config.example.toml`'s sample source list differs from the real built-in defaults, which can mislead a reader about what "default" means. |
| Annoying | `jobhunt show` output | One listing's description body was duplicated in full, with internally inconsistent employer stats between the two copies — most likely a duplicate-content artifact in the upstream posting, not deduplicated by the CLI. |
| Acceptable | Search semantics | `find --raw rust` returned nothing despite "Rust" appearing in a resume's skills section, because search only matches title/company/department/team/location/workplace fields, not description text. Correctly and clearly documented ("How search works"); flagged only because a reader skimming the worked example first could be surprised. |
| Acceptable | README structure | 2,039-line README plus a Narrow (product) / JobHunt (code) naming split adds a small orientation tax on first read — but the naming split is disclosed in the first paragraph, and headers make the document navigable. |
| Not evaluated (gap) | Install prerequisites | The clean-room environment had Rust 1.98 and a C toolchain pre-installed, so true "I don't have Rust yet" friction (no link to rustup.rs, no one-liner) was never exercised. Worth re-testing in a container with no Rust installed before the first external invite goes out. |

### Disposition: not fixed now

None of these block the experiment on the documented golden path (default
config, no `--config` flag). Per BRU-315's instructions, only issues that
clearly prevent the experiment from functioning get fixed immediately;
everything above is recorded as evidence instead, with one exception
(see [Small repository changes made](#small-repository-changes-made-this-pr)).
Recommended, not done here: fix the `--config <nonexistent-path>` hard
error (should either create the file or say so instead of an OS-level
error), list the default sources in the README, and re-run the install
step in a container with no Rust toolchain present.

## 2. What we need to observe

For each participant in the first cohort:

- Did setup complete? Where, if anywhere, did they get stuck or give up?
- Time to first useful result (from clone/install to a shortlist they'd
  actually look at).
- Did Narrow surface something they found genuinely interesting — a role
  they wouldn't have found by browsing job boards themselves?
- Shortlist size vs. how many of those they actually wanted to inspect
  (a shortlist of 5 where they wanted to look at 1 is a different signal
  than one where they wanted to look at all 5).
- Did they use `save` / `reject` / `like` / `applied`? Which, how often?
- Did they run `find` again on a later day, unprompted?
- What confused them, in their own words?
- What did they ask for spontaneously, without being prompted for it?
- Did they say anything about wanting Narrow to keep running without
  their machine, notifications, sync across devices, managed email
  integration, easier deployment, or application/interview assistance?
  (Recorded, not treated as demand — see the taxonomy below.)

Not every feature request is Cloud demand, and not every piece of praise
is evidence of product-market fit. See the next section.

## 3. Useful evidence

### Strong evidence

- A user installs Narrow and uses it repeatedly across more than one
  session, unprompted.
- A user finds an opportunity through Narrow they say they would
  otherwise have missed.
- A user voluntarily returns (runs `find` again on a later day without
  being asked to).
- A user explicitly says the setup or day-to-day operation of running
  Narrow themselves is the specific reason they'd prefer a hosted
  version.
- Multiple, independent users request the same operational convenience
  without prompting each other.

### Weak evidence

- GitHub stars, page views, "cool project" reactions.
- Hypothetical willingness to pay ("I'd probably pay for this").
- One user casually mentioning that notifications "sound useful" when
  asked a leading question, without it being something they hit as a
  real limitation in use.

### Negative evidence

- The shortlist is not obviously better than manually browsing job
  boards for that user.
- Onboarding is too hard to complete without help.
- Users end up browsing the underlying job list instead of trusting
  Narrow's shortlist (a sign the ranking/shortlisting isn't earning
  trust).
- Users do not return after the first session.
- Users do not understand what the product is, even after using it.
- No Cloud-shaped friction appears at all across the cohort.

Do not rationalize weak signals into strong ones after the fact. If the
evidence collected is mostly weak or negative, the correct conclusion is
"not yet," not "let's build Cloud anyway."

## Cloud-signal taxonomy

For every piece of feedback, place it in one category, and for anything
that could plausibly be Cloud-related, record all four fields.

Categories: discovery quality · profile/import · preference setup ·
installation/build · data/source coverage · always-on/background
execution · notifications · sync/multi-device · managed integrations ·
email/application tracking · application preparation · interview
preparation · privacy/trust · performance · documentation ·
self-hosting complexity · other.

For a potential Cloud signal, record:

1. What the user was actually trying to do.
2. What friction they experienced.
3. Their workaround, if any.
4. Whether they explicitly asked for hosted operation, or whether this
   is an inference.
5. Whether the problem could instead be fixed in OSS.

**Critical rule: do not turn a bad OSS experience into an artificial
Cloud opportunity.** If something should simply be easy in the local
product, the fix belongs in OSS, later, as its own piece of work — not as
justification for Cloud. Narrow already ships a substantial,
already-built `jobhunt-cloud` (hosted API, web app, sync — see
[docs/cloud.md](cloud.md)) with **no billing, plans, entitlements, or
premium gates** (confirmed: the README's "Known limitations" section
lists billing as "not built yet"). That existing code is an
implementation detail, not evidence of demand — it does not lower the bar
for what counts as validation, and this experiment does not treat "Cloud
already has code" as a reason to promote it. The question this experiment
answers is whether *users*, not the existing codebase, justify pushing on
it further.

## Feedback mechanism

**GitHub Issues (already enabled on the repo) with a focused feedback
form**, added at `.github/ISSUE_TEMPLATE/experiment-feedback.yml` in this
change. Issues are public, so the form's questions are about product
experience (what happened, what confused you, what you'd change), never
about resume contents, application status, or identity.

GitHub Discussions is currently disabled on the repo and was intentionally
**not** enabled as part of this change — issues are enough for a 5–10
person cohort and avoid a live settings change to the public repo.
Revisit if the cohort grows enough that threaded discussion becomes
useful.

**No private feedback channel exists yet.** SECURITY.md's private
vulnerability reporting path (GitHub private vulnerability reporting) is
for security reports only and is explicitly not repurposed here. Anyone
who wants to share sensitive context privately currently has no
documented path — this is a known gap. A dedicated alias (e.g.
`feedback@narrow.fyi`) is a reasonable follow-up once one exists, but is
out of scope for this change and not something to invent a mailbox for
today.

## Privacy

Participants are actively job searching; some of that is sensitive. Feedback
prompts (in the issue template and in outreach) ask about the *product
experience*, never for:

- resume contents
- recruiter emails or names
- salary details tied to identity
- application thread contents
- personal addresses
- immigration / work-authorization details

The issue template says this explicitly at the top, and reminds
participants that GitHub issues are public.

## First-user onboarding path

The README's existing "First run" and "The loop" sections already cover
steps 1–6 of the flow (what Narrow does → install → init → preferences →
`find` → decide on results with `save`/`reject`/`like`/`applied`) well;
nothing there needed duplicating. The one missing step was "tell us what
happened" — added as a short **Feedback** section in `README.md`, right
before **License**, linking to the issue template.

## Outreach drafts

For manual, individual use — **not** for posting anywhere or mass
messaging without explicit approval. All three share one tone: ask for
where it fails, not for praise.

**A developer you know**

> Hey — I've been building an open-source job search tool called Narrow.
> It's not another job board: it reads company job boards directly,
> checks whether you're actually eligible, and gives you a short list of
> what's worth your time instead of hundreds of listings to scroll. It
> runs entirely on your machine (Rust CLI, one SQLite file, no account
> needed). Would you be up for trying it for a real job search and
> telling me where it breaks or annoys you? I care much more about
> friction than compliments. Repo: https://github.com/bernacle/jobhunt

**An OSS/community post**

> Sharing an OSS project I've been working on: Narrow, a local-first job
> search agent (Rust CLI + optional MCP server). It searches company job
> boards directly, verifies listings are actually open, checks basic
> eligibility, and narrows down to a handful of opportunities worth
> reviewing instead of an endless feed — no account, no cloud dependency,
> Apache-2.0. Looking for a few people who are actively job hunting to
> try it against a real search and tell me what's confusing or missing.
> Feedback (especially the critical kind) welcome via GitHub issues:
> https://github.com/bernacle/jobhunt

**Someone actively job hunting**

> I'm testing an open-source tool I built called Narrow — it searches job
> boards for you and tries to only show you the few roles actually worth
> your time, instead of you scrolling hundreds. It's free, runs on your
> own machine, and takes about 5 minutes to set up with your resume. If
> you're job hunting right now, would you try it for a real search? I'd
> rather hear about what's broken or confusing than get compliments —
> that's genuinely more useful to me at this stage.

## Metrics

Kept deliberately minimal, and no code changes were made for this.

- **Local CLI (the product this experiment tests): no telemetry.** This
  is a deliberate privacy property of the local product ("Everything
  lives on your machine") and should stay that way; adding usage tracking
  to the OSS CLI to serve this experiment would violate the thing we're
  trying to validate. What can be learned without it: self-reported
  behavior (did you run `find` again?), and what a participant chooses to
  say in the issue template.
- **JobHunt Cloud already has privacy-safe, aggregate usage events**
  (`login`, `sync_pull`/`push`, `find`, `verify`, `feedback`,
  `preferences`, `mcp_tool`, and web events like `feed_opened`,
  `opportunity_viewed`, `dismiss`, `resume_imported` — see
  [docs/cloud.md § Observability and usage events](cloud.md#observability-and-usage-events)).
  This is irrelevant to this experiment's cohort unless a participant
  chooses to use Cloud, since the cohort is meant to test the OSS,
  local-first path first.
- No proposal to add resume/job-search content tracking, now or later.
- If, after the first checkpoint, self-reported feedback isn't enough to
  answer "did setup complete" or "did they return," the smallest useful
  addition would be a single opt-in, anonymous, local counter file the
  CLI can print on request (e.g. `jobhunt doctor` already prints
  diagnostics) — not a network call. That would be a separate, small
  proposal, reviewed on its own, only if evidence shows it's needed.

## OSS vs Cloud decision framework

### Evidence for hosted Cloud

Multiple, independent users say something equivalent to:

- "I don't want to keep this running."
- "Can this search while my laptop is off?"
- "I want this on multiple devices."
- "I don't want to manage Gmail OAuth myself."
- "Can it notify me when something good appears?"
- "I want application monitoring to run continuously."

### NOT evidence for hosted Cloud

- Install docs are unclear or incomplete.
- A local command's behavior is confusing.
- Configuration names or defaults are unclear.
- A bug prevents startup or produces a wrong result.
- Dependency setup (Rust, a C compiler) is annoying.

These are OSS product problems. Fix them in OSS; do not let them
accumulate into a rationale for Cloud.

## What requires real users before BRU-315 can progress

Everything in [Useful evidence](#3-useful-evidence) and the [decision
framework](#oss-vs-cloud-decision-framework) requires actual external
people actively job searching, using Narrow against their own real job
search, more than once if possible. The clean-room pass above is
usability testing only — it confirms the golden path isn't broken, not
that anyone besides the maintainer wants this. No amount of further
agent-driven testing substitutes for that. No testimonials, adoption
numbers, or Cloud demand should be asserted until real cohort data exists.

## Suggested next checkpoint

After 5 external users have completed onboarding and run at least one
real `find` against their own job search (or after 2–3 weeks, whichever
comes first): review the issue-template feedback collected so far against
the evidence definitions above, and decide only one of: (a) continue
gathering signal with the same cohort size, (b) widen the invite list, or
(c) note early Cloud-shaped signal for future discussion. Do **not** use
that checkpoint to decide whether Cloud should exist — that verdict needs
more than 5 people.
