import type { Metadata } from "next";
import Link from "next/link";

import { changeStage } from "@/app/actions";
import { StageControl } from "@/components/stage-control";
import { PageHeader } from "@/components/ui";
import { api, loadOrNoProfile } from "@/lib/api";
import type { PipelineEntryView, PipelineStage } from "@/lib/api-types";
import { STAGE_LABEL, ago } from "@/lib/format";

export const metadata: Metadata = { title: "Applications" };

const GROUPS: { stage: PipelineStage; title: string; empty?: string }[] = [
  { stage: "offer", title: "Offers" },
  { stage: "interviewing", title: "Interviewing" },
  { stage: "applied", title: "Applied" },
  { stage: "saved", title: "Saved", empty: "Nothing saved. Save opportunities from Today to keep them here." },
];

function Row({ entry, now }: { entry: PipelineEntryView; now: Date }) {
  return (
    <li className="flex flex-col gap-3 py-4 sm:flex-row sm:items-start sm:justify-between">
      <div className="min-w-0">
        <p className="font-medium">
          <Link href={`/opportunities/${entry.id}`} className="hover:underline">
            {entry.title}
          </Link>
        </p>
        <p className="text-sm text-muted">
          {entry.company} · {STAGE_LABEL[entry.stage]}
          {entry.since && <> {ago(entry.since, now)}</>}
          {entry.listing_closed && <span className="text-caution"> · listing closed</span>}
        </p>
        {entry.last_reason && (
          <p className="mt-1 text-sm text-muted">
            Your note: <q>{entry.last_reason}</q>
          </p>
        )}
      </div>
      <div className="shrink-0">
        <StageControl id={entry.id} title={entry.title} company={entry.company} stage={entry.stage} change={changeStage} />
      </div>
    </li>
  );
}

export default async function ApplicationsPage() {
  const pipeline = await loadOrNoProfile(() => api.pipeline(true));
  const now = new Date();
  const entries = pipeline === "no_profile" ? [] : pipeline.entries;
  const active = entries.filter((e) => e.stage !== "rejected");
  const rejected = entries.filter((e) => e.stage === "rejected");
  return (
    <div>
      <PageHeader title="Applications">
        What you saved and where each application stands. Not a CRM: just the stage and what changed.
      </PageHeader>
      {active.length === 0 && (
        <p className="mb-8 rounded-xl border border-line bg-surface px-5 py-6 text-muted">
          Nothing here yet. When something on{" "}
          <Link href="/today" className="underline">
            Today
          </Link>{" "}
          is worth keeping, save it or mark it applied.
        </p>
      )}
      {GROUPS.map((group) => {
        const rows = active.filter((e) => e.stage === group.stage);
        if (rows.length === 0) return null;
        return (
          <section key={group.stage} aria-labelledby={`${group.stage}-heading`} className="mb-8">
            <h2 id={`${group.stage}-heading`} className="font-serif text-xl">
              {group.title} <span className="text-base text-muted">({rows.length})</span>
            </h2>
            <ul className="mt-2 divide-y divide-line border-y border-line">
              {rows.map((e) => (
                <Row key={e.id} entry={e} now={now} />
              ))}
            </ul>
          </section>
        );
      })}
      {rejected.length > 0 && (
        <details className="mt-10">
          <summary className="cursor-pointer text-sm text-muted">Not for me ({rejected.length})</summary>
          <ul className="mt-2 divide-y divide-line border-y border-line">
            {rejected.map((e) => (
              <Row key={e.id} entry={e} now={now} />
            ))}
          </ul>
        </details>
      )}
    </div>
  );
}
