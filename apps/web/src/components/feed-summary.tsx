import type { DiscoveryStatus as Discovery, FeedSummary as Summary } from "@/lib/api-types";
import { ago, inAbout } from "@/lib/format";

import { SystemStatus } from "./ui";

function n(value: number): string {
  return value.toLocaleString("en");
}

/**
 * From every open job to what is on the page, using only the counts the
 * API computed. A stage with nothing to say is left out.
 */
export function FeedSummary({ summary }: { summary: Summary }) {
  if (summary.checked === 0) return null;
  const parts = [
    `Checked ${n(summary.checked)} open ${summary.checked === 1 ? "job" : "jobs"}`,
    `${n(summary.passed_eligibility)} you could take`,
    `${n(summary.worth_reviewing)} looked promising`,
  ];
  const onPage = summary.shown === 1 ? "1 is worth your attention today" : `${summary.shown} are worth your attention today`;
  return (
    <p className="text-[14px] text-fg-secondary nr-tnum">
      {parts.join(" · ")}
      {summary.shown > 0 && (
        <>
          {" · "}
          <span className="text-fg">{onPage}</span>
        </>
      )}
    </p>
  );
}

/** When job boards were last read and when they will be next. */
export function DiscoveryStatus({ discovery, now }: { discovery: Discovery; now?: Date }) {
  const parts = [
    discovery.last_read_at && `Checked ${ago(discovery.last_read_at, now)}`,
    discovery.next_read_at && `next check ${inAbout(discovery.next_read_at, now)}`,
  ].filter(Boolean);
  if (parts.length === 0) return null;
  return <SystemStatus>{parts.join(" · ")}</SystemStatus>;
}
