import Link from "next/link";

import type { FeedView } from "@/lib/api-types";
import { ago, inAbout } from "@/lib/format";

/**
 * Nothing new is worth the person's time. Said plainly, with what JobHunt
 * is doing meanwhile, and never padded with weaker matches.
 */
export function CaughtUp({ feed, now }: { feed: FeedView; now?: Date }) {
  const { discovery, pipeline, summary } = feed;
  const gathering = summary.checked === 0;
  const inPipeline = pipeline.saved + pipeline.applied + pipeline.interviewing + pipeline.offer;
  return (
    <section aria-labelledby="caught-up" className="rounded-xl border border-line bg-surface px-6 py-10 text-center">
      <h2 id="caught-up" className="font-serif text-3xl">
        {gathering ? "JobHunt is still gathering jobs." : "You're caught up."}
      </h2>
      <p className="mx-auto mt-3 max-w-md text-muted">
        {gathering
          ? "It reads company job boards in the background. Recommendations appear here as soon as there is something worth your time."
          : "Nothing new is worth your time right now. JobHunt keeps checking job boards in the background and will put anything good here."}
      </p>
      {(discovery.last_read_at || discovery.next_read_at) && (
        <p className="mt-4 text-sm text-muted">
          {discovery.last_read_at && <>Job boards last read {ago(discovery.last_read_at, now)}</>}
          {discovery.last_read_at && discovery.next_read_at && " · "}
          {discovery.next_read_at && <>next check {inAbout(discovery.next_read_at, now)}</>}
        </p>
      )}
      {inPipeline > 0 && (
        <p className="mt-6 text-sm">
          <Link href="/applications" className="underline">
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
      )}
    </section>
  );
}
