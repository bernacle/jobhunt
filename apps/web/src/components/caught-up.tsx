import Link from "next/link";

import type { FeedView } from "@/lib/api-types";
import { ago, inAbout } from "@/lib/format";

import { Label, textLinkClass } from "./ui";

/**
 * Nothing new is worth the person's time: a successful outcome, said
 * plainly, with what Narrow is doing meanwhile. Never padded with weaker
 * matches, never "0 jobs found".
 */
export function CaughtUp({ feed, now }: { feed: FeedView; now?: Date }) {
  const { discovery, pipeline, summary } = feed;
  const gathering = summary.checked === 0;
  const inPipeline = pipeline.saved + pipeline.applied + pipeline.interviewing + pipeline.offer;
  return (
    <section aria-labelledby="caught-up" className="mt-16 max-w-[560px] max-sm:mt-12">
      <h2 id="caught-up" className="text-display-m max-sm:text-[28px] max-sm:leading-[1.15]">
        {gathering ? "Narrow is still gathering jobs." : "You're caught up."}
      </h2>
      <p className="mt-3.5 text-body-l text-pretty text-fg-body max-sm:text-[15px]">
        {gathering
          ? "It reads company job boards in the background. Recommendations appear here as soon as there is something worth your time."
          : "Nothing new is worth your time right now. Narrow keeps checking job boards in the background and will put anything good here."}
      </p>
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
