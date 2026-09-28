import type { DiscoveryStatus as Discovery, FeedSummary as Summary } from "@/lib/api-types";
import { ago, inAbout } from "@/lib/format";

import { SystemStatus } from "./ui";

function n(value: number): string {
  return value.toLocaleString("en");
}

/**
 * What Today amounts to, from the counts the API computed: how many of
 * the open jobs checked are on the page. Nothing is estimated.
 */
export function FeedSummary({ summary }: { summary: Summary }) {
  if (summary.checked === 0) return null;
  const checked = `${n(summary.checked)} open ${summary.checked === 1 ? "job" : "jobs"}`;
  return (
    <p className="text-[14px] text-fg-secondary nr-tnum">
      {summary.shown > 0 ? (
        <>
          <span className="text-fg">{n(summary.shown)}</span> of the {checked} Narrow checked {summary.shown === 1 ? "is" : "are"} worth your attention today.
        </>
      ) : (
        <>Checked {checked}.</>
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
