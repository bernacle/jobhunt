import Link from "next/link";

import type { FeedView } from "@/lib/api-types";
import { ago, inAbout } from "@/lib/format";

import { StateMessage } from "./summary";
import { LinkButton, textLinkClass } from "./ui";

/**
 * Why Today is empty, from what the API counted: no job boards read yet,
 * none of the open jobs worth the person's time, or everything worth it
 * already dealt with. Three different situations, never one vague line.
 */
export function emptyState(feed: FeedView): { kind: "gathering" | "nothing_worthwhile" | "caught_up"; title: string; body: string } {
  const { summary, pipeline, passed_over } = feed;
  const decided = passed_over + pipeline.saved + pipeline.applied + pipeline.interviewing + pipeline.offer;
  if (summary.checked === 0) {
    return {
      kind: "gathering",
      title: "Narrow is still gathering jobs.",
      body: "Recommendations appear here as soon as there is something worth your time.",
    };
  }
  if (summary.worth_reviewing === 0 && decided === 0) {
    return {
      kind: "nothing_worthwhile",
      title: "Nothing worth your time yet.",
      body: `Narrow checked ${summary.checked.toLocaleString("en-US")} open jobs and none fits well enough to show.`,
    };
  }
  return {
    kind: "caught_up",
    title: "You're caught up.",
    body: "You've been through everything worth your time for now.",
  };
}

/**
 * Nothing new is worth the person's time: said plainly, with when Narrow
 * looks next and one useful next step. Never padded with weaker matches,
 * never "0 jobs found".
 */
export function CaughtUp({ feed, now }: { feed: FeedView; now?: Date }) {
  const { discovery, pipeline } = feed;
  const inPipeline = pipeline.saved + pipeline.applied + pipeline.interviewing + pipeline.offer;
  const { kind, title, body } = emptyState(feed);
  const when = [
    discovery.last_read_at && `Job boards last read ${ago(discovery.last_read_at, now)}`,
    discovery.next_read_at && `next check ${inAbout(discovery.next_read_at, now)}`,
  ].filter(Boolean);
  const action =
    kind === "caught_up" && inPipeline > 0 ? (
      <Link href="/applications" className={`text-[14px] ${textLinkClass}`}>
        {[
          pipeline.saved && `${pipeline.saved} saved`,
          pipeline.applied && `${pipeline.applied} applied`,
          pipeline.interviewing && `${pipeline.interviewing} interviewing`,
          pipeline.offer && `${pipeline.offer} with an offer`,
        ]
          .filter(Boolean)
          .join(" · ")}
      </Link>
    ) : kind === "nothing_worthwhile" ? (
      <LinkButton href="/preferences" className="max-sm:h-11">
        Review preferences
      </LinkButton>
    ) : undefined;
  return (
    <div className="mt-12 max-sm:mt-8">
      <StateMessage kind={kind === "gathering" ? "gathering" : kind === "caught_up" ? "done" : "empty"} title={title} id="caught-up" action={action}>
        <p>{body}</p>
        {when.length > 0 && <p className="mt-3 font-mono text-mono-s text-fg-muted">{when.join(" · ")}</p>}
      </StateMessage>
    </div>
  );
}
