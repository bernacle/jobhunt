import Link from "next/link";

import type { FeedView } from "@/lib/api-types";
import { ago, inAbout } from "@/lib/format";

import { Label, textLinkClass } from "./ui";

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
      body: "It reads company job boards in the background. Recommendations appear here as soon as there is something worth your time.",
    };
  }
  if (summary.worth_reviewing === 0 && decided === 0) {
    return {
      kind: "nothing_worthwhile",
      title: "Nothing worth your time yet.",
      body: `Narrow checked ${summary.checked.toLocaleString("en-US")} open jobs and none fits well enough to show. It keeps checking job boards in the background and will put anything good here.`,
    };
  }
  return {
    kind: "caught_up",
    title: "You're caught up.",
    body: "You've been through everything worth your time for now. Narrow keeps checking job boards in the background and will put anything new here.",
  };
}

/**
 * Nothing new is worth the person's time: a successful outcome, said
 * plainly, with what Narrow is doing meanwhile. Never padded with weaker
 * matches, never "0 jobs found".
 */
export function CaughtUp({ feed, now }: { feed: FeedView; now?: Date }) {
  const { discovery, pipeline } = feed;
  const inPipeline = pipeline.saved + pipeline.applied + pipeline.interviewing + pipeline.offer;
  const { title, body } = emptyState(feed);
  return (
    <section aria-labelledby="caught-up" className="mt-16 max-w-[560px] max-sm:mt-12">
      <h2 id="caught-up" className="text-display-m max-sm:text-[28px] max-sm:leading-[1.15]">
        {title}
      </h2>
      <p className="mt-3.5 text-body-l text-pretty text-fg-body max-sm:text-[15px]">{body}</p>
      {(discovery.last_read_at || discovery.next_read_at) && (
        <p className="mt-6 font-mono text-mono-s text-fg-muted">
          {discovery.last_read_at && <>Job boards last read {ago(discovery.last_read_at, now)}</>}
          {discovery.last_read_at && discovery.next_read_at && " · "}
          {discovery.next_read_at && <>next check {inAbout(discovery.next_read_at, now)}</>}
        </p>
      )}
      {inPipeline > 0 && (
        <div className="mt-14 border-t border-line-subtle pt-4 max-sm:mt-10">
          <Label>In progress</Label>
          <p className="mt-2 text-[14px]">
            <Link href="/applications" className={textLinkClass}>
              {[
                pipeline.saved && `${pipeline.saved} saved`,
                pipeline.applied && `${pipeline.applied} applied`,
                pipeline.interviewing && `${pipeline.interviewing} interviewing`,
                pipeline.offer && `${pipeline.offer} with an offer`,
              ]
                .filter(Boolean)
                .join(" · ")}
            </Link>
          </p>
        </div>
      )}
    </section>
  );
}
