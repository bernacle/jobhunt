import Link from "next/link";

import type { ApplicationContext, Fact, JobDetail } from "@/lib/api-types";
import { STAGE_LABEL, TIER_LABEL, ago, sentence, unscopedRemote, verificationMark } from "@/lib/format";

import { DecisionBrief, DecisionSummary } from "./decision-brief";
import { DetailActions } from "./detail-actions";
import { EligibilityDetail } from "./eligibility-detail";
import { EvidenceFact, EvidenceSection } from "./evidence";
import { EvidenceTrigger } from "./evidence-panel";
import type { FeedbackActionsProps } from "./feedback-actions";
import { CheckedLine, Provenance } from "./provenance";
import { SummaryRow, SummarySection } from "./summary";
import { CompanyTile, Consideration, EligibilityFact, Inferred, ListingFact, TierLabel, VerificationGlyph, VerifiedCheck } from "./trust";
import { Notice, textLinkClass } from "./ui";

/** Locations shown in the key facts before the rest go behind "All locations". */
const PLACES_SHOWN = 3;

const WORKPLACE: Record<string, string> = { remote: "Remote", hybrid: "Hybrid", onsite: "On-site" };

const ACTION: Record<string, string> = {
  save: "Saved",
  unsave: "Removed from saved",
  reject: "Not for me",
  applied: "Applied",
  interview: "Interviewing",
  offer: "Offer",
  like: "Liked",
  dislike: "Disliked",
};

const USABLE: Record<string, string> = {
  confirmed_by_you: "confirmed by you",
  entered_by_you: "entered by you",
  quoted_from_resume: "quoted from your resume",
};

function humanize(value: string | null | undefined): string | null {
  if (!value) return null;
  return sentence(value.replaceAll("_", " "));
}

function Evidence({ context }: { context: ApplicationContext }) {
  const facts: { fact: Fact; about: string }[] = [...context.relevant_experience, ...context.relevant_projects].flatMap((group) =>
    group.facts.slice(0, 2).map((fact) => ({ fact, about: group.label })),
  );
  const shown = facts.slice(0, 4);
  return (
    <div>
      <h3 className="text-label text-fg-muted">Your evidence for it</h3>
      <p className="mt-1 mb-2.5 text-caption text-fg-muted">Only facts you confirmed or entered, or that quote your resume.</p>
      {shown.length === 0 ? (
        <p className="text-[13px] text-fg-muted">No usable evidence matches this role yet.</p>
      ) : (
        <ul role="list" className="flex flex-col gap-3">
          {shown.map(({ fact, about }) => (
            <CheckedLine
              key={fact.claim_id}
              glyph={
                fact.usable_because === "quoted_from_resume" ? (
                  <span aria-hidden="true" className="text-fg-muted">
                    ·
                  </span>
                ) : (
                  <VerifiedCheck />
                )
              }
              when={`${about} · ${USABLE[fact.usable_because] ?? fact.usable_because}`}
            >
              {fact.text}
            </CheckedLine>
          ))}
        </ul>
      )}
      {context.missing_evidence.length > 0 && (
        <ul role="list" className="mt-3 space-y-1.5">
          {context.missing_evidence.map((m) => (
            <Consideration key={m} kind="missing" size="sm">
              {sentence(m)}
            </Consideration>
          ))}
        </ul>
      )}
      <p className="mt-3 text-[13px]">
        {context.withheld.needs_review > 0 && (
          <span className="text-fg-secondary">
            {context.withheld.needs_review} more {context.withheld.needs_review === 1 ? "claim needs" : "claims need"} your review.{" "}
          </span>
        )}
        <Link href="/profile/review" className={textLinkClass}>
          Review in Profile
        </Link>
      </p>
    </div>
  );
}

/**
 * One opportunity as a decision brief. The conclusion first: what it is,
 * Narrow's verdict, the strongest distinct reasons and the most material
 * concern. Then the few facts that decide it, each said once on one shared
 * row geometry (the evidence behind each a labelled action away), then
 * what to do. The description and the person's history follow; every
 * reason, check and source stays one action away, never on the default
 * view.
 */
export function OpportunityBrief({
  job,
  context,
  actions,
  now = new Date(),
}: {
  job: JobDetail;
  /** The person's usable evidence for it (read-only), when there is a profile. */
  context: ApplicationContext | null;
  actions: FeedbackActionsProps["actions"];
  now?: Date;
}) {
  const decision = job.decision;
  const stage = job.pipeline.stage;
  const meta = [job.department, job.team].filter(Boolean).join(" · ");
  const eligibility = job.eligibility ?? { status: "not_checked" as const, headline: "" };
  const listing = job.apply_url ?? job.url;
  const pay = job.compensation;
  const sources = job.sources ?? [];
  // One judgement of how current the listing is, for every check on the page.
  const mark = verificationMark(job.verification);
  const about = `${job.company} · ${job.title}`;

  const places = job.locations;
  const where = unscopedRemote(places, job.workplace) ? (
    <Inferred>Remote · region not stated</Inferred>
  ) : places.length > 0 ? (
    places.slice(0, PLACES_SHOWN).join(" · ")
  ) : (
    "Location not stated"
  );
  const placeText = places.join(" ").toLowerCase();
  const arrangement = [
    job.workplace && !placeText.includes(job.workplace) ? (WORKPLACE[job.workplace] ?? humanize(job.workplace)) : null,
    humanize(job.employment),
  ].filter(Boolean);
  const payPublished = pay.status === "published" && pay.ranges.length > 0;
  const payKnown = payPublished || (pay.status === "published" && Boolean(pay.summary));
  const morePay = payPublished && (pay.ranges.length > 1 || Boolean(pay.summary));

  return (
    <article aria-labelledby="title" className="max-lg:pb-40">
      <div className="sticky top-0 z-20 -mx-5 -mt-6 flex h-[52px] items-center gap-4 border-b border-line-subtle bg-ground px-5 md:-mx-10 md:-mt-8 md:px-10 lg:-mx-16 lg:-mt-11 lg:px-16">
        <nav aria-label="Breadcrumb" className="min-w-0">
          <ol className="flex min-w-0 items-center gap-2 text-ui-m text-fg-muted">
            <li className="shrink-0">
              <Link href="/today" className="flex min-h-11 items-center text-fg-secondary hover:text-fg lg:min-h-0">
                <span aria-hidden="true" className="mr-1 sm:hidden">
                  ‹
                </span>
                Today
              </Link>
            </li>
            <li aria-hidden="true" className="max-sm:hidden">
              /
            </li>
            <li aria-current="page" className="min-w-0 truncate text-fg-body max-sm:hidden">
              {job.company}
            </li>
          </ol>
        </nav>
      </div>

      <div className="max-w-[860px]">
        <header className="mt-10 max-sm:mt-5">
          <div className="flex items-center justify-between gap-4">
            <div className="flex min-w-0 items-center gap-3">
              <CompanyTile name={job.company} />
              <p className="min-w-0 truncate text-[13.5px] font-medium text-fg-secondary">
                {job.company}
                {meta && <span className="text-fg-muted max-sm:hidden"> · {meta}</span>}
              </p>
            </div>
            {decision && <TierLabel tier={decision.tier} />}
          </div>
          <h1 id="title" className="mt-4 text-display-m text-pretty [overflow-wrap:anywhere] max-sm:mt-2.5 max-sm:text-[24px] max-sm:leading-[1.2]">
            {job.title}
          </h1>
        </header>

        {decision ? (
          <section aria-labelledby="brief-heading" className="mt-5 max-w-[68ch] max-sm:mt-4">
            <h2 id="brief-heading" className="sr-only">
              Decision brief
            </h2>
            <p className="text-[17px] leading-[1.5] font-medium tracking-[-0.01em] text-pretty text-fg max-sm:text-[16px]">{sentence(decision.verdict)}</p>
            {/* The verdict already says what to check first. */}
            <DecisionSummary
              input={{ why: decision.worth, caveats: decision.caveats, unknowns: decision.unknowns, eligibilityHeadline: eligibility.headline }}
              size="lg"
              className="mt-3"
            />
            <div className="mt-2.5">
              <EvidenceTrigger
                label="Full reasoning"
                title="Why Narrow surfaced it"
                subtitle={about}
                conclusion={
                  <>
                    <span className="font-semibold text-fg">{TIER_LABEL[decision.tier]}.</span> Narrow read it as {decision.summary}.
                  </>
                }
              >
                <DecisionBrief
                  why={decision.worth}
                  caveats={decision.caveats}
                  unknowns={decision.unknowns}
                  checkFirst={decision.recommendation !== "recommended" ? decision.recommendation_note : null}
                  eligibilityHeadline={eligibility.headline}
                />
                {decision.history.length > 0 && (
                  <EvidenceSection label="Your history with it">
                    <ul className="flex flex-col gap-1 text-[14px] text-fg-body">
                      {decision.history.map((h) => (
                        <li key={h}>{sentence(h)}</li>
                      ))}
                    </ul>
                  </EvidenceSection>
                )}
                {context && <Evidence context={context} />}
              </EvidenceTrigger>
            </div>
          </section>
        ) : (
          <div className="mt-6">
            <Notice title="No decision brief yet">Upload a resume so Narrow can check this against your profile.</Notice>
          </div>
        )}

        <section aria-labelledby="facts-heading" className="mt-8 max-sm:mt-6">
          <h2 id="facts-heading" className="sr-only">
            Key facts
          </h2>
          <div className="border-b border-line-subtle">
            <SummaryRow
              id="fact-pay"
              label="Pay"
              unset={!payKnown}
              value={payPublished ? pay.ranges[0] : payKnown ? <q>{pay.summary}</q> : pay.status === "not_published" ? "Not published" : "Unknown"}
              importance={
                payPublished ? (
                  pay.verified ? (
                    <>
                      <VerificationGlyph mark={mark} /> confirmed at the source{pay.verified_at && ` ${ago(pay.verified_at, now)}`}
                    </>
                  ) : (
                    "as listed, not confirmed at the source"
                  )
                ) : payKnown ? (
                  "as published"
                ) : (
                  "Unknown, not low"
                )
              }
              action={
                morePay && (
                  <EvidenceTrigger label="As published" srLabel="pay" title="Pay as published" subtitle={about}>
                    <EvidenceSection label="Every figure the listing publishes">
                      {pay.ranges.map((r, i) => (
                        <EvidenceFact key={r} label={i === 0 ? "Published" : "Also published"}>
                          <span className="nr-tnum">{r}</span>
                        </EvidenceFact>
                      ))}
                      {pay.summary && (
                        <EvidenceFact label="In its words">
                          <q>{pay.summary}</q>
                        </EvidenceFact>
                      )}
                    </EvidenceSection>
                    <p className="text-[13px] text-fg-secondary">
                      {pay.verified ? `Confirmed at the source${pay.verified_at ? ` ${ago(pay.verified_at, now)}` : ""}.` : "As listed, not confirmed at the source."} Narrow
                      never converts currencies or periods.
                    </p>
                  </EvidenceTrigger>
                )
              }
            />
            <SummaryRow
              id="fact-location"
              label="Location"
              unset={places.length === 0 && !job.workplace}
              value={where}
              importance={arrangement.length > 0 ? arrangement.join(" · ") : undefined}
              action={
                places.length > PLACES_SHOWN && (
                  <EvidenceTrigger label="All locations" title="Where the listing says" subtitle={about}>
                    <ul className="flex flex-col gap-1.5 text-[14px] text-fg-body">
                      {places.map((l) => (
                        <li key={l}>{l}</li>
                      ))}
                    </ul>
                    <p className="text-[13px] text-fg-muted">As the listing states them. Whether you can work from them is in Eligibility.</p>
                  </EvidenceTrigger>
                )
              }
            />
            <SummaryRow
              id="fact-eligibility"
              label="Eligibility"
              unset={!job.eligibility}
              value={job.eligibility ? <EligibilityFact eligibility={eligibility} /> : "Not checked"}
              importance={job.eligibility ? job.eligibility.option : "there is no profile to check against"}
              action={
                job.eligibility && (
                  <EvidenceTrigger
                    label="Checks"
                    srLabel="for eligibility"
                    title="Eligibility checks"
                    subtitle={about}
                    conclusion={
                      <>
                        <EligibilityFact eligibility={eligibility} />
                        {job.eligibility.option && <span className="text-fg-muted"> · {job.eligibility.option}</span>}
                      </>
                    }
                  >
                    <EligibilityDetail detail={job.eligibility} withSummary={false} />
                  </EvidenceTrigger>
                )
              }
            />
            <SummaryRow
              id="fact-listing"
              label="Listing"
              value={<ListingFact verification={job.verification} now={now} />}
              importance={job.posted_at ? `posted ${ago(job.posted_at, now)}` : undefined}
              action={
                <EvidenceTrigger label="Sources" srLabel="of the listing" title="Where it's listed" subtitle={about}>
                  <EvidenceSection label="Verification">
                    <ul role="list" className="flex flex-col gap-3">
                      <CheckedLine glyph={<VerificationGlyph mark={mark} />}>
                        <ListingFact verification={job.verification} now={now} />
                      </CheckedLine>
                      {job.verification.not_trusted_because && (
                        <CheckedLine glyph={<VerificationGlyph mark={null} />}>{sentence(job.verification.not_trusted_because)}</CheckedLine>
                      )}
                      {payPublished && (
                        <CheckedLine
                          glyph={<VerificationGlyph mark={pay.verified ? mark : null} />}
                          when={pay.verified_at ? `Checked ${ago(pay.verified_at, now)}` : undefined}
                        >
                          {pay.verified ? "Pay confirmed at the source" : "Pay as listed, not confirmed at the source"}
                        </CheckedLine>
                      )}
                    </ul>
                  </EvidenceSection>
                  <EvidenceSection label={`${job.source_count} ${job.source_count === 1 ? "source" : "sources"}`}>
                    {sources.length > 0 ? (
                      <Provenance sources={sources} verification={job.verification} now={now} />
                    ) : (
                      <p className="text-[13px] text-fg-muted">No source record is available.</p>
                    )}
                  </EvidenceSection>
                  <p className="text-[14px]">
                    <a href={listing} target="_blank" rel="noopener noreferrer" className={textLinkClass}>
                      Open original posting<span className="sr-only"> (opens a new tab)</span> ↗
                    </a>
                  </p>
                </EvidenceTrigger>
              }
            />
          </div>
        </section>

        <div className="mt-5">
          <DetailActions id={job.id} title={job.title} company={job.company} stage={stage} actions={actions} />
        </div>

        <div className="mt-10 border-b border-line-subtle max-sm:mt-8">
          <SummarySection
            id="description"
            label="Description"
            replaces
            conclusion={
              job.description ? (
                <p className="line-clamp-3 whitespace-pre-line text-fg-body">{job.description}</p>
              ) : (
                <span className="text-fg-muted">The listing has no description.</span>
              )
            }
            more={job.description ? "Full description" : undefined}
            aside={
              <a href={listing} target="_blank" rel="noopener noreferrer" className={`text-[13px] ${textLinkClass} max-sm:inline-flex max-sm:min-h-11 max-sm:items-center`}>
                Open original posting<span className="sr-only"> (opens a new tab)</span> ↗
              </a>
            }
          >
            {job.description && (
              <>
                <div className="max-w-[68ch] text-body-m whitespace-pre-line text-fg-body [overflow-wrap:anywhere]">{job.description}</div>
                <p className="mt-3.5 text-[12.5px] text-fg-muted">
                  {job.description_truncated ? "Only the start of the description is available. " : ""}Shown as published. Narrow hasn&apos;t
                  rewritten it.
                </p>
              </>
            )}
          </SummarySection>

          {job.pipeline.feedback.length > 0 && (
            <SummarySection
              id="history"
              label="Your history"
              conclusion={
                <>
                  <span className="font-medium text-fg">{STAGE_LABEL[stage]}</span>
                  {job.pipeline.since && <span className="text-fg-muted"> since {ago(job.pipeline.since, now)}</span>}
                </>
              }
              more="All activity"
            >
              <ul role="list" className="flex flex-col gap-2.5">
                {job.pipeline.feedback.map((f) => (
                  <li key={f.id} className="grid grid-cols-[72px_minmax(0,1fr)] gap-2.5 text-[13px] leading-[1.45]">
                    <span className="pt-px font-mono text-mono-s text-fg-muted">{ago(f.at, now)}</span>
                    <span className="text-fg-body">
                      {ACTION[f.action] ?? humanize(f.action)}
                      {f.reason && (
                        <>
                          {" "}
                          <span className="text-fg-secondary">
                            · <q>{f.reason}</q>
                          </span>
                        </>
                      )}
                    </span>
                  </li>
                ))}
              </ul>
            </SummarySection>
          )}
        </div>
      </div>
    </article>
  );
}
