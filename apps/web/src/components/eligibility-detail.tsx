import type { EligibilityDetail as Detail } from "@/lib/api-types";
import { sentence } from "@/lib/format";

import { Consideration, EligibilityFact } from "./trust";

const RULE: Record<string, string> = {
  listing: "Listing",
  work_mode: "Work mode",
  presence: "Presence",
  country_constraint: "Location",
  region_constraint: "Region",
  remote_scope: "Remote scope",
  authorization: "Work authorization",
  engagement: "Engagement",
  timezone: "Time zone",
  ambiguity: "Consistency",
};

const VERDICT: Record<string, { word: string; marker: string }> = {
  pass: { word: "Fine", marker: "bg-fg" },
  conditional: { word: "On a condition", marker: "bg-warning" },
  unknown: { word: "Unclear", marker: "border border-fg-secondary" },
  fail: { word: "Rules you out", marker: "bg-danger" },
};

function ruleLabel(rule: string): string {
  return RULE[rule] ?? sentence(rule.replaceAll("_", " "));
}

/**
 * The eligibility decision, one requirement per row: what the posting says
 * (its own words) and what that means for the person. A pass is a solid
 * square, a condition sand, an unclear reading hollow; the verdict is
 * always also a word.
 */
export function EligibilityDetail({ detail }: { detail: Detail }) {
  const reasons = detail.reasons.filter((r) => r.verdict !== "not_applicable");
  // Laid out by the width this column actually has (a container query),
  // not the viewport's: beside the aside at 1024px it is phone-narrow.
  return (
    <div className="@container">
      <p className="text-[14px]">
        <EligibilityFact eligibility={{ status: detail.status, headline: detail.headline }} />
        {detail.option && <span className="text-fg-muted"> ({detail.option})</span>}
      </p>
      {reasons.length > 0 && (
        <>
          <div
            aria-hidden="true"
            className="mt-4 hidden grid-cols-[120px_minmax(0,1fr)_minmax(0,190px)] gap-4 border-b border-line-subtle pb-2 text-label text-fg-muted @xl:grid"
          >
            <span>Requirement</span>
            <span>Posting says</span>
            <span>You</span>
          </div>
          <ul role="list" className="@max-xl:mt-3 @max-xl:border-t @max-xl:border-line-subtle">
            {reasons.map((r) => {
              const verdict = VERDICT[r.verdict] ?? { word: r.verdict, marker: "border border-fg-muted" };
              const evidence = r.evidence ?? [];
              return (
                <li
                  key={`${r.rule}-${r.conclusion}`}
                  data-eligibility-row className="grid gap-x-4 gap-y-1 border-b border-line-subtle py-3 text-row @xl:grid-cols-[120px_minmax(0,1fr)_minmax(0,190px)] @xl:items-baseline"
                >
                  <span className="font-medium text-fg-secondary @xl:font-normal">{ruleLabel(r.rule)}</span>
                  <span className="text-fg-body">
                    {/* Stacked, the columns' header is gone: say which is which. */}
                    <span className="text-fg-muted @xl:sr-only">Posting says: </span>
                    {evidence.length > 0 ? (
                      evidence.map((e) => (
                        <q key={e} className="block">
                          {e}
                        </q>
                      ))
                    ) : (
                      <span className="text-fg-muted">Not stated in the posting</span>
                    )}
                  </span>
                  <span className="flex items-baseline gap-[9px] text-fg">
                    <span aria-hidden="true" className={`size-1.5 shrink-0 -translate-y-px rounded-[1px] ${verdict.marker}`} />
                    <span>
                      <span className="font-medium">{verdict.word}</span>
                      <span className="text-fg-secondary"> · {sentence(r.conclusion)}</span>
                    </span>
                  </span>
                </li>
              );
            })}
          </ul>
        </>
      )}
      {(detail.conflicts ?? []).length > 0 && (
        <div className="mt-4">
          <p className="text-[14px] font-medium">The posting contradicts itself</p>
          <ul role="list" className="mt-1.5 space-y-1.5">
            {(detail.conflicts ?? []).map((c) => (
              <Consideration key={c} kind="caution">
                {c}
              </Consideration>
            ))}
          </ul>
        </div>
      )}
      <p className="mt-3 text-caption text-fg-muted">{detail.disclaimer}</p>
    </div>
  );
}
