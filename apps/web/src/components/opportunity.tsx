"use client";

import Link from "next/link";
import { useState } from "react";

import type { FeedItem } from "@/lib/api-types";
import { type DecisionInput, selectDecision } from "@/lib/decision";
import { TIER_LABEL, sentence, sourceLabel } from "@/lib/format";

import { ConcernLine, DecisionBrief, DecisionSummary } from "./decision-brief";
import { EvidenceFact, EvidenceSection } from "./evidence";
import { EvidenceTrigger } from "./evidence-panel";
import { FeedbackActions, type FeedbackActionsProps, type Outcome, OutcomeLine } from "./feedback-actions";
import { CompanyTile, EligibilityFact, FactRow, PayFact, PlaceFact, TierLabel, VerificationStamp, VerificationStatus } from "./trust";
import { Label, Raised, textLinkClass } from "./ui";

type Props = { item: FeedItem; actions: FeedbackActionsProps["actions"]; now?: Date };

function context(item: FeedItem): string {
  return [item.department, item.team].filter(Boolean).join(" · ");
}

/** The feed item's explanation, as the decision selection reads it. */
function decisionInput(item: FeedItem): DecisionInput {
  const unknowns = item.unknowns ?? [];
  return {
    why: item.why,
    caveats: item.consider.filter((c) => !unknowns.includes(c)),
    unknowns,
    checkFirst: item.recommendation !== "recommended" ? item.recommendation_note : null,
    eligibilityHeadline: item.eligibility.headline,
  };
}

function changed(item: FeedItem): boolean {
  return item.reason === "changed" && (item.changes?.length ?? 0) > 0;
}

function Changed({ item }: { item: FeedItem }) {
  if (!changed(item)) return null;
  return (
    <p className="flex gap-2.5 text-[14px] text-fg-body">
      <span aria-hidden="true" className="mt-[0.6em] size-[5px] shrink-0 rounded-[1px] bg-info" />
      <span>
        <span className="font-medium text-fg">Changed since you looked:</span> {item.changes!.join("; ")}
      </span>
    </p>
  );
}

/** Once an action went through, the opportunity folds into one line. */
function Done({ item, outcome, headingId, compact = false }: { item: FeedItem; outcome: Outcome; headingId: string; compact?: boolean }) {
  return (
    <article
      aria-labelledby={headingId}
      className={
        compact
          ? "flex animate-[nr-fade_180ms_var(--nr-ease-out)] flex-col gap-1 border-t border-line-subtle pt-4 pb-5 @2xl:row-span-7"
          : "flex animate-[nr-fade_180ms_var(--nr-ease-out)] flex-col gap-1 border-b border-line-subtle py-3.5 sm:flex-row sm:items-baseline sm:justify-between sm:gap-4"
      }
    >
      <h2 id={headingId} className={`min-w-0 text-[14px] font-semibold ${compact ? "" : "truncate"}`}>
        {item.title} <span className="font-normal text-fg-muted">· {item.company}</span>
      </h2>
      <div role="status" className={`text-[13px] text-fg-secondary ${compact ? "" : "sm:text-right"}`}>
        <OutcomeLine outcome={outcome} />
      </div>
    </article>
  );
}

/**
 * The company's other recommendations, which Today holds back so that one
 * company doesn't fill it: one line, opened on demand. They stay new (not
 * shown), and opening one doesn't change that.
 */
export function MoreAtCompany({ item }: { item: FeedItem }) {
  const others = item.also_at_company ?? [];
  if (others.length === 0) return null;
  return (
    <details className="group text-[13px]">
      <summary className="inline-flex cursor-pointer list-none items-center gap-1.5 font-medium text-fg-secondary hover:text-fg max-sm:min-h-11 [&::-webkit-details-marker]:hidden">
        <span aria-hidden="true" className="font-mono text-mono-s text-fg-muted transition-transform group-open:rotate-90">
          ›
        </span>
        +{others.length} more {others.length === 1 ? "role" : "roles"} at {item.company}
      </summary>
      <OtherRoles item={item} />
    </details>
  );
}

function OtherRoles({ item }: { item: FeedItem }) {
  return (
    <ul className="mt-1.5 space-y-1 pl-4 text-[13.5px]">
      {(item.also_at_company ?? []).map((o) => (
        <li key={o.id}>
          <Link href={`/opportunities/${o.id}`} className="text-fg-body underline decoration-line underline-offset-2 hover:text-fg max-sm:inline-flex max-sm:min-h-11 max-sm:items-center">
            {o.title}
          </Link>
        </li>
      ))}
    </ul>
  );
}

/**
 * Everything Today knows about one opportunity, on demand: every reason
 * and concern the API returned, the facts with how far they're confirmed,
 * what changed, verification and where it rests, and the way to the full
 * brief.
 */
function Evidence({ item, now }: { item: FeedItem; now?: Date }) {
  const input = decisionInput(item);
  const others = item.also_at_company ?? [];
  const payConfirmed = item.compensation.status === "published" && item.compensation.ranges.length > 0;
  return (
    <EvidenceTrigger
      label="See evidence"
      srLabel={`for ${item.title} at ${item.company}`}
      title="Why Narrow surfaced it"
      subtitle={`${item.company} · ${item.title}`}
      conclusion={
        <>
          <span className="font-semibold text-fg">{TIER_LABEL[item.tier]}.</span> {sentence(item.summary)}.
        </>
      }
    >
      <DecisionBrief {...input} />
      <EvidenceSection label="Facts">
        <EvidenceFact label="Eligibility">
          <EligibilityFact eligibility={item.eligibility} />
        </EvidenceFact>
        <EvidenceFact label="Pay">
          <PayFact compensation={item.compensation} />
          {payConfirmed && <span className="text-fg-muted">{item.compensation.verified ? " · confirmed at the source" : " · not confirmed at the source"}</span>}
        </EvidenceFact>
        <EvidenceFact label="Location">
          {(item.locations?.length ?? 0) > 2 ? item.locations!.join(" · ") : <PlaceFact locations={item.locations} workplace={item.workplace} />}
        </EvidenceFact>
        {item.sources > 1 && <EvidenceFact label="Listed on">{item.sources} boards</EvidenceFact>}
      </EvidenceSection>
      {changed(item) && (
        <EvidenceSection label="Changed since you looked">
          <ul className="flex flex-col gap-1 text-[14px] text-fg-body">
            {item.changes!.map((c) => (
              <li key={c}>{sentence(c)}</li>
            ))}
          </ul>
        </EvidenceSection>
      )}
      <EvidenceSection label="Verification">
        <VerificationStamp verification={item.verification} now={now} />
        {item.verification.source && <p className="font-mono text-mono-s text-fg-muted">{sourceLabel(item.verification.source)}</p>}
      </EvidenceSection>
      {others.length > 0 && (
        <EvidenceSection label={`Also at ${item.company}`}>
          <OtherRoles item={item} />
        </EvidenceSection>
      )}
      <p className="text-[14px]">
        <Link href={`/opportunities/${item.id}`} className={textLinkClass}>
          Open the full brief
        </Link>
      </p>
    </EvidenceTrigger>
  );
}

/**
 * Today's lead: the one raised surface on the page. The opportunity, the
 * facts that decide it, its two strongest reasons and its most material
 * concern, how current it is, then what to do. Everything else is one
 * labelled action away. No score, ever.
 */
export function OpportunityLead({ item, actions, now }: Props) {
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const headingId = `opp-${item.id}`;
  if (outcome) return <Done item={item} outcome={outcome} headingId={headingId} />;
  const meta = context(item);
  return (
    <Raised as="article" aria-labelledby={headingId} className="px-8 pt-7 pb-5 max-md:px-6 max-sm:px-[18px] max-sm:pt-5 max-sm:pb-[18px]">
      <div className="flex items-center justify-between gap-4">
        <div className="flex min-w-0 items-center gap-3 max-sm:gap-2.5">
          <CompanyTile name={item.company} />
          <p className="min-w-0 truncate text-[13.5px] font-medium text-fg-secondary max-sm:text-[13px]">
            {item.company}
            {meta && <span className="text-fg-muted max-sm:hidden"> · {meta}</span>}
          </p>
        </div>
        <TierLabel tier={item.tier} />
      </div>
      <h2 id={headingId} className="mt-4 text-title-l text-pretty max-sm:mt-3 max-sm:text-[21px] max-sm:leading-[1.25]">
        <Link href={`/opportunities/${item.id}`} className="hover:text-fg-body">
          {item.title}
        </Link>
      </h2>
      <FactRow
        className="mt-3"
        items={[
          <PayFact key="p" compensation={item.compensation} />,
          <PlaceFact key="l" locations={item.locations} workplace={item.workplace} />,
          <EligibilityFact key="e" eligibility={item.eligibility} />,
        ]}
      />
      <div className="mt-4 empty:hidden">
        <Changed item={item} />
      </div>
      <div className="mt-5 border-t border-line-subtle pt-5 max-sm:mt-4 max-sm:pt-4">
        <DecisionSummary input={decisionInput(item)} variant="lead" size="lg" />
        <div className="mt-2.5 flex flex-wrap items-center gap-x-5">
          <Evidence item={item} now={now} />
          <MoreAtCompany item={item} />
        </div>
      </div>
      <div className="mt-4 flex flex-wrap items-center justify-between gap-4 border-t border-line-subtle pt-4 max-sm:flex-col max-sm:items-stretch max-sm:gap-3">
        <VerificationStatus verification={item.verification} now={now} withSource />
        <FeedbackActions id={item.id} title={item.title} company={item.company} actions={actions} onDone={setOutcome} variant="lead" />
      </div>
    </Raised>
  );
}

/**
 * A peer: a comparison object on the ground. Seven slots (header, title,
 * facts, reason, concern, verification, actions) share their rows with the
 * other peers beside it (CSS subgrid), so the same kind of information
 * starts in the same place and the actions sit on one line. Stacked on a
 * narrow screen, the reserved space and an empty concern slot go away.
 */
export function OpportunityPeer({ item, actions, now }: Props) {
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const headingId = `opp-${item.id}`;
  if (outcome) return <Done item={item} outcome={outcome} headingId={headingId} compact />;
  const { reasons, concerns } = selectDecision(decisionInput(item), "peer");
  const reason = reasons[0];
  const concern = concerns[0];
  return (
    <article
      aria-labelledby={headingId}
      className="flex flex-col border-t border-line-subtle pt-4 @2xl:row-span-7 @2xl:grid @2xl:grid-rows-subgrid @2xl:gap-y-0"
    >
      <div className="flex min-w-0 justify-between gap-3 pb-1.5 text-ui-s text-fg-muted">
        <span className="min-w-0 truncate">
          {item.company}
          {changed(item) && <span className="text-fg-secondary"> · changed</span>}
        </span>
        <TierLabel tier={item.tier} className="text-[12.5px]" />
      </div>
      <h2 id={headingId} className="pb-2 text-title-m text-pretty @2xl:line-clamp-3 @2xl:min-h-[calc(2.6em+8px)]">
        <Link href={`/opportunities/${item.id}`} className="hover:text-fg-body">
          {item.title}
        </Link>
      </h2>
      <div className="flex flex-col gap-px pb-3 text-[13.5px] leading-[1.45]">
        <p className="font-medium">
          <PayFact compensation={item.compensation} />
        </p>
        <p className="text-fg-body">
          <PlaceFact locations={item.locations} workplace={item.workplace} />
        </p>
        <p className="text-fg-body">
          <EligibilityFact eligibility={item.eligibility} brief />
        </p>
      </div>
      <div className="pb-2">
        {reason ? (
          <p className="text-row text-pretty text-fg-body @2xl:line-clamp-2">
            <span className="sr-only">Why it may be worth your time: </span>
            {sentence(reason)}
          </p>
        ) : (
          <p className="text-row text-fg-muted">Nothing specific to you yet.</p>
        )}
      </div>
      <div className={concern ? "pb-3" : "hidden @2xl:block"}>
        {concern && (
          <ul role="list">
            <ConcernLine concern={concern} size="sm" />
          </ul>
        )}
      </div>
      <div className="flex flex-wrap items-baseline justify-between gap-x-3 pb-1.5">
        <VerificationStatus verification={item.verification} now={now} />
        <Evidence item={item} now={now} />
      </div>
      <div className="pb-5 @2xl:self-end">
        <FeedbackActions id={item.id} title={item.title} company={item.company} actions={actions} onDone={setOutcome} variant="peer" />
      </div>
    </article>
  );
}

/**
 * Today's list: the lead, then its peers side by side. Every opportunity
 * keeps its own state (a folded outcome) under its own id, so when a
 * refreshed feed puts a different opportunity first, nothing from the old
 * lead carries over.
 */
export function TodayFeed({ items, actions, now }: { items: FeedItem[]; actions: FeedbackActionsProps["actions"]; now?: Date }) {
  const [lead, ...peers] = items;
  if (!lead) return null;
  // Two or four peers pair up; three share a row where there's room.
  const columns = peers.length === 2 || peers.length === 4 ? "@2xl:grid-cols-2" : peers.length >= 3 ? "@2xl:grid-cols-2 @4xl:grid-cols-3" : "";
  return (
    <ol aria-label="Recommendations">
      <li key={lead.id}>
        <OpportunityLead key={lead.id} item={lead} actions={actions} now={now} />
      </li>
      {peers.length > 0 && (
        <li className="@container mt-11 max-sm:mt-8">
          <Label className="mb-2">Also worth a look</Label>
          <ol aria-label="Also worth a look" className={`grid gap-x-10 ${columns}`}>
            {peers.map((item) => (
              <li key={item.id} className="min-w-0 @2xl:row-span-7 @2xl:grid @2xl:grid-rows-subgrid">
                <OpportunityPeer key={item.id} item={item} actions={actions} now={now} />
              </li>
            ))}
          </ol>
        </li>
      )}
    </ol>
  );
}
