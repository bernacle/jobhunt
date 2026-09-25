import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";

import { putAside, recordFeedback } from "@/app/actions";
import { DecisionBrief } from "@/components/decision-brief";
import { EligibilityDetail } from "@/components/eligibility-detail";
import { DetailActions } from "@/components/detail-actions";
import { TierMark } from "@/components/tier";
import { Notice, Section } from "@/components/ui";
import { ApiError, api, load } from "@/lib/api";
import type { ApplicationContext, JobDetail } from "@/lib/api-types";
import { STAGE_LABEL, ago, authorityLabel, compensationLine, placeLine, verificationLine } from "@/lib/format";

export const metadata: Metadata = { title: "Opportunity" };

const IN_PROGRESS = ["saved", "applied", "interviewing", "offer"];

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

export default async function OpportunityPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const job = await detail(id);
  const now = new Date();
  const stage = job.pipeline.stage;
  let context: ApplicationContext | null = null;
  if (IN_PROGRESS.includes(stage)) {
    context = await load(() => api.applicationContext(id)).catch(() => null);
  }
  const decision = job.decision;
  const pay = compensationLine(job.compensation);
  const facts = [
    placeLine(job.locations, job.workplace),
    job.department,
    job.team,
    job.employment?.replace("_", " "),
    job.posted_at && `posted ${ago(job.posted_at, now)}`,
  ].filter(Boolean);

  return (
    <article aria-labelledby="title">
      <Link href="/today" className="text-sm text-muted underline hover:text-ink">
        ← Today
      </Link>
      <header className="mt-6">
        <p className="text-sm font-medium text-muted">{job.company}</p>
        <h1 id="title" className="mt-1 font-serif text-3xl leading-tight tracking-tight sm:text-4xl">
          {job.title}
        </h1>
        <p className="mt-2 text-sm text-muted">{facts.join(" · ")}</p>
        {decision && (
          <div className="mt-4 flex flex-wrap items-baseline gap-x-3 gap-y-1">
            <TierMark tier={decision.tier} />
            <p className="text-[0.95rem]">{decision.verdict}</p>
          </div>
        )}
      </header>

      {decision?.recommendation_note && decision.recommendation !== "recommended" && (
        <div className="mt-4">
          <Notice tone="caution" title="Check first">
            {decision.recommendation_note}
          </Notice>
        </div>
      )}

      {decision ? (
        <div className="mt-8">
          <DecisionBrief why={decision.worth} consider={decision.caveats} unknowns={decision.unknowns} headingLevel={2} />
        </div>
      ) : (
        <div className="mt-8">
          <Notice title="No decision brief yet">Upload a resume so JobHunt can check this against your profile.</Notice>
        </div>
      )}

      <div className="mt-8 flex flex-wrap items-center justify-between gap-4 border-y border-line py-4">
        <DetailActions
          id={job.id}
          title={job.title}
          company={job.company}
          stage={stage}
          actions={{ feedback: recordFeedback, putAside }}
        />
        <a
          href={job.apply_url ?? job.url}
          target="_blank"
          rel="noopener noreferrer"
          className="text-sm underline"
        >
          Open the listing<span className="sr-only"> (opens a new tab)</span> ↗
        </a>
      </div>

      <Section title="Pay" id="pay">
        <p className={pay.known ? "" : "text-muted"}>{pay.text}</p>
        {job.compensation.ranges.length > 1 && (
          <ul className="mt-1 text-sm text-muted">
            {job.compensation.ranges.slice(1).map((r) => (
              <li key={r}>{r}</li>
            ))}
          </ul>
        )}
        {job.compensation.summary && (
          <p className="mt-1 text-sm text-muted">
            As published: <q>{job.compensation.summary}</q>
          </p>
        )}
        <p className="mt-1 text-sm text-muted">
          {job.compensation.verified
            ? `Checked at the source ${ago(job.compensation.verified_at, now)}.`
            : "From the listing when JobHunt found it; not confirmed at the source."}
        </p>
      </Section>

      <Section title="Can you take it?" id="eligibility">
        {job.eligibility ? (
          <EligibilityDetail detail={job.eligibility} />
        ) : (
          <p className="text-muted">Not checked: there is no profile to check against.</p>
        )}
      </Section>

      <Section title="Is it real and open?" id="verification">
        <p className={job.verification.trusted ? "" : "text-caution"}>{verificationLine(job.verification, now)}</p>
        {job.sources && job.sources.length > 0 && (
          <ul className="mt-3 divide-y divide-line rounded-lg border border-line">
            {job.sources.map((s) => (
              <li key={s.job_id} className="px-4 py-3 text-sm">
                <p className="flex flex-wrap justify-between gap-2">
                  <a href={s.url} target="_blank" rel="noopener noreferrer" className="font-medium underline">
                    {s.source}
                    <span className="sr-only"> (opens a new tab)</span>
                  </a>
                  <span className="text-muted">{s.status === "open" ? "listed" : "closed"}</span>
                </p>
                <p className="mt-1 text-muted">
                  Published by {authorityLabel(s.authority)} · first seen {ago(s.first_seen_at, now)} · last seen{" "}
                  {ago(s.last_seen_at, now)}
                  {s.last_success_at && <> · verified {ago(s.last_success_at, now)}</>}
                </p>
                {s.last_attempt?.failure && <p className="mt-1 text-caution">Last check failed: {s.last_attempt.failure}</p>}
              </li>
            ))}
          </ul>
        )}
      </Section>

      <Section title="The role" id="role">
        {job.description ? (
          <div className="whitespace-pre-line text-[0.95rem] leading-relaxed">{job.description}</div>
        ) : (
          <p className="text-muted">The listing has no description.</p>
        )}
      </Section>

      {job.pipeline.feedback.length > 0 && (
        <Section title="Your history" id="history">
          <p className="text-sm">
            Now: <span className="font-medium">{STAGE_LABEL[stage]}</span>
            {job.pipeline.since && <span className="text-muted"> since {ago(job.pipeline.since, now)}</span>}
          </p>
          <ul className="mt-2 space-y-1 text-sm text-muted">
            {job.pipeline.feedback.map((f) => (
              <li key={f.id}>
                {ago(f.at, now)}: {f.action}
                {f.reason && (
                  <>
                    {" "}
                    — <q className="text-ink">{f.reason}</q>
                  </>
                )}
              </li>
            ))}
          </ul>
        </Section>
      )}

      {context && (
        <Section
          title="What you can point to"
          id="evidence"
          description="Only facts you confirmed or that are quoted from your resume. Use them as written."
        >
          {context.relevant_experience.length === 0 && context.job_asks.technologies.length === 0 ? (
            <p className="text-sm text-muted">No approved evidence matches this role yet.</p>
          ) : (
            <ul className="space-y-4 text-sm">
              {context.relevant_experience.map((group) => (
                <li key={group.id}>
                  <p className="font-medium">
                    {group.label}
                    {group.period && <span className="font-normal text-muted"> · {group.period}</span>}
                  </p>
                  {group.relevance.length > 0 && <p className="text-muted">Relevant for: {group.relevance.join(", ")}</p>}
                  <ul className="mt-1 list-disc pl-5">
                    {group.facts.slice(0, 4).map((f) => (
                      <li key={f.claim_id}>{f.text}</li>
                    ))}
                  </ul>
                </li>
              ))}
            </ul>
          )}
          {context.missing_evidence.length > 0 && (
            <p className="mt-4 text-sm text-muted">
              No evidence yet for: {context.missing_evidence.join(", ")}.
            </p>
          )}
          {context.withheld.needs_review > 0 && (
            <p className="mt-2 text-sm text-muted">
              {context.withheld.needs_review} more {context.withheld.needs_review === 1 ? "claim needs" : "claims need"} your review
              before they can be used.{" "}
              <Link href="/profile#review" className="underline">
                Review them
              </Link>
              .
            </p>
          )}
          <p className="mt-4 text-sm text-muted">
            Want help with the application? Ask your AI assistant (it uses the same evidence).{" "}
            <Link href="/settings#assistant" className="underline">
              Connect one
            </Link>
            .
          </p>
        </Section>
      )}
    </article>
  );
}
