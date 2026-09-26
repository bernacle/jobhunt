"use client";

import Link from "next/link";
import { useState } from "react";

import type { FeedItem } from "@/lib/api-types";

import { DecisionBrief } from "./decision-brief";
import { FeedbackActions, type FeedbackActionsProps, type Outcome, OutcomeLine } from "./feedback-actions";
import { CompanyTile, EligibilityFact, FactRow, PayFact, PlaceFact, TierLabel, VerificationStamp } from "./trust";
import { Label, Raised } from "./ui";

type Props = { item: FeedItem; actions: FeedbackActionsProps["actions"]; now?: Date };

function context(item: FeedItem): string {
  return [item.department, item.team].filter(Boolean).join(" · ");
}

function facts(item: FeedItem) {
  return [
    <EligibilityFact key="e" eligibility={item.eligibility} />,
    <PayFact key="p" compensation={item.compensation} />,
    <PlaceFact key="l" locations={item.locations} workplace={item.workplace} />,
    item.sources > 1 ? (
      <span key="s" className="text-fg-secondary">
        Listed on {item.sources} boards
      </span>
    ) : null,
  ];
}

function Changed({ item }: { item: FeedItem }) {
  if (item.reason !== "changed" || (item.changes?.length ?? 0) === 0) return null;
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
function Done({ item, outcome, headingId }: { item: FeedItem; outcome: Outcome; headingId: string }) {
  return (
    <article
      aria-labelledby={headingId}
      className="flex animate-[nr-fade_180ms_var(--nr-ease-out)] flex-col gap-1 border-b border-line-subtle py-3.5 sm:flex-row sm:items-baseline sm:justify-between sm:gap-4"
    >
      <h2 id={headingId} className="min-w-0 truncate text-[14px] font-semibold">
        {item.title} <span className="font-normal text-fg-muted">· {item.company}</span>
      </h2>
      <div role="status" className="text-[13px] text-fg-secondary sm:text-right">
        <OutcomeLine outcome={outcome} />
      </div>
    </article>
  );
}

function checkFirst(item: FeedItem): string | null {
  return item.recommendation !== "recommended" ? (item.recommendation_note ?? null) : null;
}

/**
 * Today's lead: the one raised surface on the page. The opportunity, the
 * facts that decide it, why it may be worth the person's time, what to
 * consider, how current it is, then what to do. No score, ever.
 */
export function OpportunityLead({ item, actions, now }: Props) {
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const headingId = `opp-${item.id}`;
  if (outcome) return <Done item={item} outcome={outcome} headingId={headingId} />;
  const meta = context(item);
  return (
    <Raised as="article" aria-labelledby={headingId} className="px-8 pt-7 pb-[22px] max-md:px-6 max-sm:px-[18px] max-sm:pt-5 max-sm:pb-[18px]">
      <div className="flex items-center justify-between gap-4">
        <div className="flex min-w-0 items-center gap-3 max-sm:gap-2.5">
          <CompanyTile name={item.company} />
          <p className="min-w-0 truncate text-[13.5px] font-medium text-fg-secondary max-sm:text-[13px]">
            {item.company}
            {meta && <span className="text-fg-muted"> · {meta}</span>}
          </p>
        </div>
        <TierLabel tier={item.tier} />
      </div>
      <h2 id={headingId} className="mt-4 text-title-l text-pretty max-sm:mt-3.5 max-sm:text-[21px] max-sm:leading-[1.25]">
        <Link href={`/opportunities/${item.id}`} className="hover:text-fg-body">
          {item.title}
        </Link>
      </h2>
      <FactRow items={facts(item)} className="mt-3.5" />
      <div className="mt-4 empty:hidden">
        <Changed item={item} />
      </div>
      <DecisionBrief
        why={item.why}
        caveats={item.consider}
        unknowns={item.unknowns ?? []}
        checkFirst={checkFirst(item)}
        className="mt-6 border-t border-line-subtle pt-5 max-sm:mt-5 max-sm:pt-4"
      />
      <div className="mt-[18px] flex flex-wrap items-center justify-between gap-4 border-t border-line-subtle pt-4 max-sm:flex-col max-sm:items-stretch">
        <VerificationStamp verification={item.verification} now={now} />
        <FeedbackActions id={item.id} title={item.title} company={item.company} actions={actions} onDone={setOutcome} variant="lead" />
      </div>
    </Raised>
  );
}

/**
 * The denser peers under the lead: on the ground, divided by hairlines,
 * still with the facts, the reasons and the cautions, never a bare list.
 */
export function OpportunityPeer({ item, actions, now }: Props) {
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const headingId = `opp-${item.id}`;
  if (outcome) return <Done item={item} outcome={outcome} headingId={headingId} />;
  const meta = context(item);
  return (
    <article aria-labelledby={headingId} className="flex flex-col gap-[5px] border-b border-line-subtle pt-4 pb-3.5 max-sm:gap-1.5 max-sm:pt-[18px] max-sm:pb-4">
      <div className="flex justify-between gap-3 text-ui-s text-fg-muted">
        <span className="min-w-0 truncate">
          {item.company}
          {meta && <span className="max-sm:hidden"> · {meta}</span>}
        </span>
        <TierLabel tier={item.tier} className="text-[12.5px]" />
      </div>
      <h2 id={headingId} className="text-[17px] leading-[1.3] font-semibold tracking-[-0.01em] text-pretty max-sm:text-[16px]">
        <Link href={`/opportunities/${item.id}`} className="hover:text-fg-body">
          {item.title}
        </Link>
      </h2>
      <FactRow items={facts(item)} size="sm" />
      <Changed item={item} />
      <DecisionBrief
        why={item.why}
        caveats={item.consider}
        unknowns={item.unknowns ?? []}
        checkFirst={checkFirst(item)}
        compact
        className="mt-1"
      />
      <div className="mt-1 flex flex-wrap items-center justify-between gap-x-4 gap-y-2 max-sm:flex-col max-sm:items-stretch">
        <VerificationStamp verification={item.verification} now={now} />
        <FeedbackActions id={item.id} title={item.title} company={item.company} actions={actions} onDone={setOutcome} variant="peer" />
      </div>
    </article>
  );
}

/**
 * Today's list: the lead, then its peers. Every opportunity keeps its own
 * state (a folded outcome) under its own id, so when a refreshed feed puts
 * a different opportunity first, nothing from the old lead carries over.
 */
export function TodayFeed({ items, actions, now }: { items: FeedItem[]; actions: FeedbackActionsProps["actions"]; now?: Date }) {
  const [lead, ...peers] = items;
  if (!lead) return null;
  return (
    <ol aria-label="Recommendations">
      <li key={lead.id}>
        <OpportunityLead key={lead.id} item={lead} actions={actions} now={now} />
      </li>
      {peers.map((item, i) => (
        <li key={item.id} className={i === 0 ? "mt-11 max-sm:mt-8" : undefined}>
          {i === 0 && <Label className="mb-1">Also worth a look</Label>}
          <OpportunityPeer key={item.id} item={item} actions={actions} now={now} />
        </li>
      ))}
    </ol>
  );
}
