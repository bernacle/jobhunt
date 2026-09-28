import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";

import { putAside, recordFeedback } from "@/app/actions";
import { DecisionBrief, DecisionSummary } from "@/components/decision-brief";
import { DetailActions } from "@/components/detail-actions";
import { EligibilityDetail } from "@/components/eligibility-detail";
import { CheckedLine, Provenance } from "@/components/provenance";
import { SummarySection } from "@/components/summary";
import {
  CompanyTile,
  Consideration,
  EligibilityFact,
  FactRow,
  Inferred,
  PayFact,
  PlaceFact,
  TierLabel,
  VerificationGlyph,
  VerificationStatus,
  VerifiedCheck,
} from "@/components/trust";
import { FactRows, Notice, textLinkClass } from "@/components/ui";
import { ApiError, api, load } from "@/lib/api";
import type { ApplicationContext, Fact, JobDetail } from "@/lib/api-types";
import { STAGE_LABEL, ago, sentence, unscopedRemote, verificationMark } from "@/lib/format";

export const metadata: Metadata = { title: "Opportunity" };

async function detail(id: string): Promise<JobDetail> {
  try {
    return await load(() => api.opportunity(id));
  } catch (error) {
    if (error instanceof ApiError && ["unknown_opportunity", "invalid_arguments", "ambiguous_id"].includes(error.code)) {
      notFound();
    }
    throw error;
  }
}

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

export default async function OpportunityPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const job = await detail(id);
  const now = new Date();
  const decision = job.decision;
  // The evidence seam: read-only, and only when there is a profile.
  const context: ApplicationContext | null = decision ? await load(() => api.applicationContext(id)).catch(() => null) : null;
  const stage = job.pipeline.stage;
  const meta = [job.department, job.team].filter(Boolean).join(" · ");
  const eligibility = job.eligibility ?? { status: "not_checked" as const, headline: "" };
  const listing = job.apply_url ?? job.url;
  const pay = job.compensation;
  const sources = job.sources ?? [];
  // One judgement of how current the listing is, for every check on the page.
  const mark = verificationMark(job.verification);

  const where = unscopedRemote(job.locations, job.workplace) ? (
    <Inferred>Remote · region not stated</Inferred>
  ) : job.locations.length > 0 ? (
    job.locations.join(" · ")
  ) : (
    <span className="text-missing">Location not stated</span>
  );
  const placeText = job.locations.join(" ").toLowerCase();
  const arrangement = [
    job.workplace && !placeText.includes(job.workplace) ? (WORKPLACE[job.workplace] ?? humanize(job.workplace)) : null,
    humanize(job.employment),
  ].filter(Boolean);
  const payPublished = pay.status === "published" && pay.ranges.length > 0;

  return (
    <article aria-labelledby="title" className="max-lg:pb-24">
      <div className="sticky top-0 z-20 -mx-5 -mt-6 flex h-[52px] items-center gap-4 border-b border-line-subtle bg-ground px-5 md:-mx-10 md:-mt-8 md:px-10 lg:-mx-16 lg:-mt-11 lg:px-16">
        <nav aria-label="Breadcrumb" className="min-w-0">
          <ol className="flex items-center gap-2 text-ui-m text-fg-muted">
            <li>
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
            <li aria-current="page" className="truncate text-fg-body max-sm:hidden">
              {job.company}
            </li>
          </ol>
        </nav>
        <DetailActions id={job.id} title={job.title} company={job.company} stage={stage} actions={{ feedback: recordFeedback, putAside }} />
      </div>

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
        <h1 id="title" className="mt-4 text-display-m text-pretty max-sm:mt-2.5 max-sm:text-[24px] max-sm:leading-[1.2]">
          {job.title}
        </h1>
        <FactRow
          className="mt-4"
          items={[
            <PayFact key="p" compensation={pay} />,
            <PlaceFact key="l" locations={job.locations} workplace={job.workplace} />,
            <EligibilityFact key="e" eligibility={eligibility} />,
            job.employment ? (
              <span key="m" className="text-fg-secondary">
                {humanize(job.employment)}
              </span>
            ) : null,
          ]}
        />
        {decision ? (
          <section aria-labelledby="brief-heading" className="mt-6 max-w-[68ch] max-sm:mt-5">
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
          </section>
        ) : (
          <div className="mt-6">
            <Notice title="No decision brief yet">Upload a resume so Narrow can check this against your profile.</Notice>
          </div>
        )}
      </header>

      <div className="mt-10 border-b border-line-subtle max-sm:mt-8">
        <SummarySection
          id="eligibility"
          label="Eligibility"
          conclusion={
            job.eligibility ? (
              <>
                <EligibilityFact eligibility={eligibility} />
                {job.eligibility.option && <span className="text-fg-muted"> · {job.eligibility.option}</span>}
              </>
            ) : (
              <span className="text-fg-muted">Not checked: there is no profile to check against.</span>
            )
          }
          more={job.eligibility ? "Eligibility checks" : undefined}
        >
          {job.eligibility && <EligibilityDetail detail={job.eligibility} withSummary={false} />}
        </SummarySection>

        <SummarySection
          id="pay"
          label="Compensation"
          conclusion={
            payPublished ? (
              <>
                <span className="font-medium text-fg nr-tnum">{pay.ranges[0]}</span>
                {pay.verified ? (
                  <span className="text-fg-secondary">
                    {" "}
                    <VerificationGlyph mark={mark} /> confirmed at the source{pay.verified_at && ` ${ago(pay.verified_at, now)}`}
                  </span>
                ) : (
                  <span className="text-fg-secondary"> · as listed, not confirmed at the source</span>
                )}
              </>
            ) : pay.status === "published" && pay.summary ? (
              <q>{pay.summary}</q>
            ) : (
              <>
                <span className="text-missing">{pay.status === "not_published" ? "Pay not published" : "Pay unknown"}</span>
                <span className="text-fg-muted">. Unknown, not low.</span>
              </>
            )
          }
          more={payPublished && (pay.ranges.length > 1 || pay.summary) ? "All published pay" : undefined}
        >
          {payPublished && (pay.ranges.length > 1 || pay.summary) && (
            <FactRows
              labelWidth="sm:grid-cols-[120px_minmax(0,1fr)]"
              rows={[
                ...pay.ranges.map((r, i) => ({ k: i === 0 ? "Published" : "Also published", v: <span className="nr-tnum">{r}</span>, key: r })),
                ...(pay.summary ? [{ k: "As published", v: <q>{pay.summary}</q>, key: "summary" }] : []),
              ]}
            />
          )}
        </SummarySection>

        <SummarySection
          id="location"
          label="Location"
          conclusion={
            <>
              {where}
              {arrangement.length > 0 && <span className="text-fg-secondary"> · {arrangement.join(" · ")}</span>}
            </>
          }
        />

        {decision && (
          <SummarySection
            id="why"
            label="Why Narrow surfaced it"
            conclusion={
              <>
                Narrow read it as <span className="text-fg">{decision.summary}</span>.
              </>
            }
            more="Full reasoning"
          >
            <div className="flex flex-col gap-7">
              <DecisionBrief
                why={decision.worth}
                caveats={decision.caveats}
                unknowns={decision.unknowns}
                checkFirst={decision.recommendation !== "recommended" ? decision.recommendation_note : null}
                eligibilityHeadline={eligibility.headline}
              />
              {decision.history.length > 0 && (
                <div>
                  <h3 className="text-label text-fg-muted">Your history with it</h3>
                  <ul className="mt-2.5 flex flex-col gap-1 text-[14px] text-fg-body">
                    {decision.history.map((h) => (
                      <li key={h}>{sentence(h)}</li>
                    ))}
                  </ul>
                </div>
              )}
              {context && <Evidence context={context} />}
            </div>
          </SummarySection>
        )}

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
              <div className="max-w-[68ch] text-body-m whitespace-pre-line text-fg-body">{job.description}</div>
              <p className="mt-3.5 text-[12.5px] text-fg-muted">
                {job.description_truncated ? "Only the start of the description is available. " : ""}Shown as published. Narrow hasn&apos;t
                rewritten it.
              </p>
            </>
          )}
        </SummarySection>

        <SummarySection
          id="sources"
          label="Sources"
          conclusion={
            <>
              <VerificationStatus verification={job.verification} now={now} withSource className="text-[12.5px]" />
              <p className="mt-0.5 font-mono text-mono-s text-fg-muted">
                {job.source_count} {job.source_count === 1 ? "source" : "sources"}
                {job.posted_at && ` · posted ${ago(job.posted_at, now)}`}
              </p>
            </>
          }
          more="Source history"
        >
          <ul role="list" className="flex flex-col gap-3">
            {job.verification.not_trusted_because && (
              <CheckedLine glyph={<VerificationGlyph mark={null} />}>{sentence(job.verification.not_trusted_because)}</CheckedLine>
            )}
            {payPublished && (
              <CheckedLine glyph={<VerificationGlyph mark={pay.verified ? mark : null} />} when={pay.verified_at ? `Checked ${ago(pay.verified_at, now)}` : undefined}>
                {pay.verified ? "Pay confirmed at the source" : "Pay as listed, not confirmed at the source"}
              </CheckedLine>
            )}
          </ul>
          {sources.length > 0 && (
            <div className="mt-4">
              <Provenance sources={sources} verification={job.verification} now={now} />
            </div>
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
    </article>
  );
}
