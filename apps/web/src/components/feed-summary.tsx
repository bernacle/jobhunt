import type { FeedSummary as Summary } from "@/lib/api-types";

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
    <p className="text-sm text-muted">
      {parts.join(" · ")}
      {summary.shown > 0 && <> · <span className="text-ink">{onPage}</span></>}
    </p>
  );
}
