import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";
import type { ReactNode } from "react";

import { putAside, recordFeedback } from "@/app/actions";
import { DecisionBrief } from "@/components/decision-brief";
import { DetailActions } from "@/components/detail-actions";
import { EligibilityDetail } from "@/components/eligibility-detail";
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
  VerifiedCheck,
} from "@/components/trust";
import { CheckedLine, Provenance } from "@/components/provenance";
import { FactRows, Label, Notice, Raised, textLinkClass } from "@/components/ui";
import { ApiError, api, load } from "@/lib/api";
import type { ApplicationContext, Fact, JobDetail } from "@/lib/api-types";
import { STAGE_LABEL, ago, sentence, unscopedRemote, verificationLine, verificationMark } from "@/lib/format";

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

function MainSection({ id, title, description, action, children }: { id: string; title: string; description?: string; action?: ReactNode; children: ReactNode }) {
  return (
    <section aria-labelledby={`${id}-heading`} className="min-w-0">
      <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
        <h2 id={`${id}-heading`} className="text-title-m">
          {title}
        </h2>
        {action}
      </div>
      {description && <p className="mt-1 text-[13px] text-fg-muted">{description}</p>}
      <div className="mt-3.5">{children}</div>
    </section>
  );
}

function AsideSection({ id, title, caption, children }: { id: string; title: string; caption?: string; children: ReactNode }) {
  return (
    <section aria-labelledby={`${id}-heading`} className="min-w-0">
      <Label as="h2" id={`${id}-heading`}>
        {title}
      </Label>
      {caption && <p className="mt-1 text-caption text-fg-muted">{caption}</p>}
      <div className="mt-2.5">{children}</div>
    </section>
  );
}

function Evidence({ context }: { context: ApplicationContext }) {
  const facts: { fact: Fact; about: string }[] = [...context.relevant_experience, ...context.relevant_projects].flatMap((group) =>
    group.facts.slice(0, 2).map((fact) => ({ fact, about: group.label })),
  );
  const shown = facts.slice(0, 4);
  return (
    <AsideSection id="evidence" title="Evidence for your application" caption="Only facts you confirmed or entered, or that quote your resume.">
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
        <Link href="/profile#review" className={textLinkClass}>
          Review in Profile
        </Link>
      </p>
    </AsideSection>
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
              {meta && <span className="text-fg-muted"> · {meta}</span>}
            </p>
          </div>
          {decision && <TierLabel tier={decision.tier} className="sm:hidden" />}
        </div>
        <h1 id="title" className="mt-4 text-display-m text-pretty max-sm:mt-2.5 max-sm:text-[24px] max-sm:leading-[1.2]">
          {job.title}
        </h1>
        <FactRow
          className="mt-4"
          items={[
            <EligibilityFact key="e" eligibility={eligibility} />,
            <PayFact key="p" compensation={pay} />,
            <PlaceFact key="l" locations={job.locations} workplace={job.workplace} />,
            job.employment ? (
              <span key="m" className="text-fg-secondary">
                {humanize(job.employment)}
              </span>
            ) : null,
          ]}
        />
      </header>

      {decision ? (
        <Raised as="section" aria-labelledby="brief-heading" className="mt-8 px-8 pt-7 pb-[22px] max-md:px-6 max-sm:mt-5 max-sm:px-[18px] max-sm:pt-[18px] max-sm:pb-4">
          <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
            <Label as="h2" id="brief-heading">
              Decision brief
            </Label>
            <p className="flex items-baseline gap-3">
              <TierLabel tier={decision.tier} className="max-sm:hidden" />
              <span className="font-mono text-mono-s text-fg-muted">
                {job.source_count} {job.source_count === 1 ? "source" : "sources"}
                {job.posted_at && ` · posted ${ago(job.posted_at, now)}`}
              </span>
            </p>
          </div>
          <p className="mt-3 max-w-[62ch] text-lede text-pretty max-sm:text-[16px]">{sentence(decision.verdict)}</p>
          <DecisionBrief
            why={decision.worth}
            caveats={decision.caveats}
            unknowns={decision.unknowns}
            checkFirst={decision.recommendation !== "recommended" ? decision.recommendation_note : null}
            headingLevel={3}
            className="mt-6 border-t border-line-subtle pt-5 max-sm:mt-4 max-sm:pt-4"
          />
        </Raised>
      ) : (
        <div className="mt-8">
          <Notice title="No decision brief yet">Upload a resume so Narrow can check this against your profile.</Notice>
        </div>
      )}

      <div className="mt-12 grid gap-x-14 gap-y-11 max-sm:mt-9 lg:grid-cols-[minmax(0,1fr)_280px]">
        <div className="flex min-w-0 flex-col gap-11 max-sm:gap-9">
          <MainSection id="eligibility" title="Eligibility" description="Checked against your Profile. Quotes are from the posting.">
            {job.eligibility ? (
              <EligibilityDetail detail={job.eligibility} />
            ) : (
              <p className="text-[14px] text-fg-muted">Not checked: there is no profile to check against.</p>
            )}
          </MainSection>

          <MainSection id="pay" title="Compensation">
            {pay.status === "published" && pay.ranges.length > 0 ? (
              <>
                <p className="text-title-l nr-tnum max-sm:text-[22px]">{pay.ranges[0]}</p>
                <p className="mt-1.5 font-mono text-mono-s text-fg-muted">
                  {pay.verified ? (
                    <>
                      <VerificationGlyph mark={mark} /> Checked at the source {ago(pay.verified_at, now)}
                    </>
                  ) : (
                    "From the listing when Narrow found it; not confirmed at the source"
                  )}
                </p>
              </>
            ) : pay.status === "published" && pay.summary ? (
              <p className="text-body-m">{pay.summary}</p>
            ) : (
              <>
                <p className="text-title-m text-missing">{pay.status === "not_published" ? "Pay not published" : "Pay unknown"}</p>
                <p className="mt-1 text-[13px] text-fg-muted">Unknown, not low. Narrow doesn&apos;t guess it.</p>
              </>
            )}
            {(pay.ranges.length > 1 || pay.summary) && pay.ranges.length > 0 && (
              <div className="mt-4">
                <FactRows
                  labelWidth="sm:grid-cols-[120px_minmax(0,1fr)]"
                  rows={[
                    ...pay.ranges.slice(1).map((r) => ({ k: "Also published", v: <span className="nr-tnum">{r}</span>, key: r })),
                    ...(pay.summary ? [{ k: "As published", v: <q>{pay.summary}</q>, key: "summary" }] : []),
                  ]}
                />
              </div>
            )}
          </MainSection>

          <MainSection id="location" title="Location and employment">
            <FactRows
              labelWidth="sm:grid-cols-[120px_minmax(0,1fr)]"
              rows={[
                {
                  k: "Arrangement",
                  v: job.workplace ? WORKPLACE[job.workplace] ?? humanize(job.workplace) : <span className="text-missing">Not stated</span>,
                },
                {
                  k: "Where",
                  v: unscopedRemote(job.locations, job.workplace) ? (
                    <Inferred>Remote · region not stated</Inferred>
                  ) : job.locations.length > 0 ? (
                    job.locations.join(" · ")
                  ) : (
                    <span className="text-missing">Not stated</span>
                  ),
                },
                { k: "Employment", v: humanize(job.employment) ?? <span className="text-missing">Not stated</span> },
                ...(meta ? [{ k: "Team", v: meta }] : []),
                { k: "Posted", v: job.posted_at ? ago(job.posted_at, now) : <span className="text-missing">Not stated</span> },
              ]}
            />
          </MainSection>

          <MainSection
            id="description"
            title="Full description"
            action={
              <a href={listing} target="_blank" rel="noopener noreferrer" className={`text-ui-m ${textLinkClass}`}>
                Open original posting<span className="sr-only"> (opens a new tab)</span> ↗
              </a>
            }
          >
            {job.description ? (
              <div className="max-w-[68ch] text-body-m whitespace-pre-line text-fg-body">{job.description}</div>
            ) : (
              <p className="text-[14px] text-fg-muted">The listing has no description.</p>
            )}
            <p className="mt-3.5 text-[12.5px] text-fg-muted">
              {job.description_truncated ? "Only the start of the description is available. " : ""}Shown as published. Narrow
              hasn&apos;t rewritten it.
            </p>
          </MainSection>
        </div>

        <aside aria-label="About this listing" className="flex min-w-0 flex-col gap-9">
          <AsideSection id="verification" title="Verification">
            <ul role="list" className="flex flex-col gap-2.5">
              <CheckedLine
                glyph={<VerificationGlyph mark={mark} />}
                when={job.verification.verified_at ? `Last verified ${ago(job.verification.verified_at, now)}` : undefined}
              >
                {verificationLine(job.verification, now)}
              </CheckedLine>
              {pay.status === "published" && pay.ranges.length > 0 && (
                <CheckedLine
                  glyph={<VerificationGlyph mark={pay.verified ? mark : null} />}
                  when={pay.verified_at ? `Checked ${ago(pay.verified_at, now)}` : undefined}
                >
                  {pay.verified ? "Pay confirmed at the source" : "Pay as listed, not confirmed at the source"}
                </CheckedLine>
              )}
            </ul>
          </AsideSection>

          {sources.length > 0 && (
            <AsideSection id="provenance" title="Provenance">
              <Provenance sources={sources} verification={job.verification} now={now} />
            </AsideSection>
          )}

          {job.pipeline.feedback.length > 0 && (
            <AsideSection id="history" title="Your history">
              <p className="mb-2.5 text-[13px] text-fg-body">
                Now: <span className="font-medium text-fg">{STAGE_LABEL[stage]}</span>
                {job.pipeline.since && <span className="text-fg-muted"> since {ago(job.pipeline.since, now)}</span>}
              </p>
              <ul role="list" className="flex flex-col gap-2.5">
                {job.pipeline.feedback.map((f) => (
                  <li key={f.id} className="grid grid-cols-[64px_minmax(0,1fr)] gap-2.5 text-[13px] leading-[1.45]">
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
            </AsideSection>
          )}

          {context && <Evidence context={context} />}
        </aside>
      </div>
    </article>
  );
}
