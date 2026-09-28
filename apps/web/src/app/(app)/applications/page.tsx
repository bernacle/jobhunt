import type { Metadata } from "next";
import Link from "next/link";

import { changeStage } from "@/app/actions";
import { StageControl } from "@/components/stage-control";
import { StageTabs } from "@/components/stage-tabs";
import { StateMessage } from "@/components/summary";
import { LinkButton, PageHeader, StatusText } from "@/components/ui";
import { api, loadOrNoProfile } from "@/lib/api";
import type { PipelineEntryView, PipelineStage } from "@/lib/api-types";
import { ago } from "@/lib/format";

export const metadata: Metadata = { title: "Applications" };

type Status = Parameters<typeof StatusText>[0]["status"];

// The real stages of the pipeline, furthest first. Nothing is invented to
// look like a CRM.
const GROUPS: { stage: PipelineStage; tab: string; title: string; status: Status }[] = [
  { stage: "offer", tab: "offer", title: "Offer", status: "offer" },
  { stage: "interviewing", tab: "interview", title: "Interview", status: "active" },
  { stage: "applied", tab: "applied", title: "Applied", status: "waiting" },
  { stage: "saved", tab: "saved", title: "Saved", status: "waiting" },
];

function Row({ entry, now }: { entry: PipelineEntryView; now: Date }) {
  const closed = entry.listing_closed;
  return (
    <li
      className={`grid grid-cols-[minmax(0,1fr)_auto] items-baseline gap-x-4 gap-y-1 border-b border-line-subtle px-3 py-3 text-row transition-colors duration-[120ms] hover:bg-ground-hover max-sm:px-0 max-sm:hover:bg-transparent lg:min-h-12 lg:grid-cols-[minmax(0,2fr)_minmax(0,1.3fr)_72px_300px] lg:items-center lg:py-2 ${
        closed ? "text-fg-muted" : "text-fg"
      }`}
    >
      <p className="col-span-2 min-w-0 lg:col-span-1 lg:truncate">
        <Link href={`/opportunities/${entry.id}`} className="font-semibold hover:text-fg-body">
          {entry.title}
        </Link>{" "}
        <span className="text-fg-muted">· {entry.company}</span>
      </p>
      {/* Only what changed or what the person said; an open listing is the routine case. */}
      <p className={`min-w-0 empty:max-lg:hidden lg:truncate ${closed ? "text-fg-secondary" : "text-fg-body"}`}>
        {closed ? "Listing closed" : entry.last_reason ? <q>{entry.last_reason}</q> : null}
      </p>
      <span className="font-mono text-mono-s text-fg-muted">{entry.since ? ago(entry.since, now) : ""}</span>
      <div className="col-span-2 mt-1.5 lg:col-span-1 lg:mt-0">
        <StageControl id={entry.id} title={entry.title} company={entry.company} stage={entry.stage} change={changeStage} />
      </div>
    </li>
  );
}

function Group({ title, status, entries, now, id }: { title: string; status: Status; entries: PipelineEntryView[]; now: Date; id: string }) {
  return (
    <section aria-labelledby={`${id}-heading`} className="mt-7">
      <div className="flex items-center gap-2.5 border-b border-line-subtle px-3 pb-2.5 max-sm:px-0">
        <h2 id={`${id}-heading`}>
          <StatusText status={status}>{title}</StatusText>
        </h2>
        <span className="font-mono text-mono-xs text-fg-muted">{entries.length}</span>
      </div>
      <ul role="list">
        {entries.map((e) => (
          <Row key={e.id} entry={e} now={now} />
        ))}
      </ul>
    </section>
  );
}

export default async function ApplicationsPage() {
  const pipeline = await loadOrNoProfile(() => api.pipeline(true));
  const now = new Date();
  const entries = pipeline === "no_profile" ? [] : pipeline.entries;
  const active = entries.filter((e) => e.stage !== "rejected");
  const rejected = entries.filter((e) => e.stage === "rejected");
  const groups = GROUPS.map((g) => ({ ...g, entries: active.filter((e) => e.stage === g.stage) }));
  return (
    <div>
      <PageHeader title="Applications" count={active.length} />
      {active.length === 0 ? (
        <StateMessage
          kind="empty"
          title="Nothing in progress yet."
          action={
            <LinkButton href="/today" className="max-sm:h-11">
              Go to Today
            </LinkButton>
          }
        >
          Roles you save or mark as applied appear here with their stage.
        </StateMessage>
      ) : (
        <StageTabs
          tabs={[
            { id: "all", label: "All", count: active.length },
            ...[...groups].reverse().map((g) => ({ id: g.tab, label: g.title, count: g.entries.length })),
          ]}
          groups={groups
            .filter((g) => g.entries.length > 0)
            .map((g) => ({
              stage: g.tab,
              node: <Group id={g.tab} title={g.title} status={g.status} entries={g.entries} now={now} />,
            }))}
        />
      )}
      {rejected.length > 0 && (
        <details className="group mt-10">
          <summary className="flex min-h-11 cursor-pointer list-none items-center gap-2 text-ui-m text-fg-secondary hover:text-fg">
            <span aria-hidden="true" className="text-fg-muted transition-transform duration-[120ms] group-open:rotate-90">
              ›
            </span>
            Not for me ({rejected.length})
          </summary>
          <ul role="list" className="mt-2 border-t border-line-subtle">
            {rejected.map((e) => (
              <Row key={e.id} entry={e} now={now} />
            ))}
          </ul>
        </details>
      )}
    </div>
  );
}
