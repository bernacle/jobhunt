"use client";

import Link from "next/link";
import { useState } from "react";

import type { FeedItem } from "@/lib/api-types";
import { placeLine } from "@/lib/format";

import { DecisionBrief } from "./decision-brief";
import { FeedbackActions, type FeedbackActionsProps, type Outcome, OutcomeLine } from "./feedback-actions";
import { TierMark } from "./tier";
import { TrustLines } from "./trust";

/**
 * One recommendation on Today: the opportunity, why it may matter, what to
 * consider, then what to do. The tier is the ranking's coarse one; there
 * is no score on the card.
 */
export function OpportunityCard({
  item,
  actions,
  now,
}: {
  item: FeedItem;
  actions: FeedbackActionsProps["actions"];
  now?: Date;
}) {
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const headingId = `opp-${item.id}`;
  const context = [item.department, item.team].filter(Boolean).join(" · ");
  const place = placeLine(item.locations, item.workplace);

  if (outcome) {
    return (
      <article aria-labelledby={headingId} className="rounded-xl border border-line bg-sunken px-5 py-4 text-sm">
        <h2 id={headingId} className="font-medium">
          {item.title} <span className="font-normal text-muted">· {item.company}</span>
        </h2>
        <div role="status" className="mt-1 text-muted">
          <OutcomeLine outcome={outcome} />
        </div>
      </article>
    );
  }

  return (
    <article aria-labelledby={headingId} className="rounded-xl border border-line bg-surface p-5 sm:p-6">
      <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
        <p className="text-sm font-medium text-muted">{item.company}</p>
        <TierMark tier={item.tier} />
      </div>
      <h2 id={headingId} className="mt-1 font-serif text-2xl leading-snug">
        <Link href={`/opportunities/${item.id}`} className="hover:underline">
          {item.title}
        </Link>
      </h2>
      <p className="mt-1 text-sm text-muted">
        {[place, context].filter(Boolean).join(" · ")}
        {item.sources > 1 && <> · listed on {item.sources} boards</>}
      </p>

      {item.reason === "changed" && (item.changes?.length ?? 0) > 0 && (
        <p className="mt-3 rounded-md bg-accent-soft px-3 py-2 text-sm">
          <span className="font-medium">Changed since you looked:</span> {item.changes!.join("; ")}
        </p>
      )}
      {item.recommendation !== "recommended" && item.recommendation_note && (
        <p className="mt-3 rounded-md bg-caution-soft px-3 py-2 text-sm">
          <span className="font-medium">Check first:</span> {item.recommendation_note}
        </p>
      )}

      <div className="mt-5">
        <DecisionBrief why={item.why} consider={item.consider} />
      </div>

      <div className="mt-5 border-t border-line pt-4">
        <TrustLines
          compensation={item.compensation}
          eligibility={item.eligibility}
          verification={item.verification}
          now={now}
        />
      </div>

      <div className="mt-5 flex flex-wrap items-start justify-between gap-3">
        <FeedbackActions
          id={item.id}
          title={item.title}
          company={item.company}
          actions={actions}
          onDone={setOutcome}
        />
        <Link href={`/opportunities/${item.id}`} className="mt-1.5 text-sm text-muted underline hover:text-ink">
          Full brief <span className="sr-only">for {item.title}</span>
        </Link>
      </div>
    </article>
  );
}
