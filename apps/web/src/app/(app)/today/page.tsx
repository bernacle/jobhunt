import type { Metadata } from "next";
import { redirect } from "next/navigation";

import { putAside, recordFeedback } from "@/app/actions";
import { CaughtUp } from "@/components/caught-up";
import { DiscoveryStatus, FeedSummary } from "@/components/feed-summary";
import { OpportunityLead, OpportunityPeer } from "@/components/opportunity";
import { RefreshControls } from "@/components/refresh-on-focus";
import { Label, Notice, PageHeader, textLinkClass } from "@/components/ui";
import { api, loadOrNoProfile } from "@/lib/api";
import { inAbout } from "@/lib/format";

export const metadata: Metadata = { title: "Today" };

/**
 * The primary screen, and a finite one: one lead, a few peers, then an
 * explicit end. Nothing is added to make it look full.
 */
export default async function TodayPage() {
  const feed = await loadOrNoProfile(() => api.feed(5));
  if (feed === "no_profile") redirect("/welcome");
  const now = new Date(feed.generated_at);
  const date = now.toLocaleDateString("en-GB", { weekday: "long", day: "numeric", month: "long" });
  const actions = { feedback: recordFeedback, putAside };
  const [lead, ...peers] = feed.items;
  const next = feed.discovery.next_read_at;
  return (
    <div>
      <PageHeader
        title="Today"
        aside={
          <time dateTime={feed.generated_at} className="font-mono text-mono-s text-fg-muted">
            {date}
          </time>
        }
      />
      <div className="-mt-5 mb-8 flex flex-wrap items-center gap-x-4 gap-y-1.5 max-sm:-mt-3.5 max-sm:mb-5">
        <DiscoveryStatus discovery={feed.discovery} now={now} />
        <FeedSummary summary={feed.summary} />
        <RefreshControls />
      </div>
      {!feed.learning.has_preferences && (
        <div className="mb-8">
          <Notice title="Tell Narrow what you want">
            Recommendations get sharper once you say which roles, teams and pay you&apos;re after.{" "}
            <a href="/preferences" className={textLinkClass}>
              Add preferences
            </a>
          </Notice>
        </div>
      )}
      {feed.caught_up || !lead ? (
        <CaughtUp feed={feed} now={now} />
      ) : (
        <>
          <ol aria-label="Recommendations">
            <li>
              <OpportunityLead item={lead} actions={actions} now={now} />
            </li>
            {peers.map((item, i) => (
              <li key={item.id} className={i === 0 ? "mt-11 max-sm:mt-8" : undefined}>
                {i === 0 && <Label className="mb-1">Also worth a look</Label>}
                <OpportunityPeer item={item} actions={actions} now={now} />
              </li>
            ))}
          </ol>
          <div className="mt-9 flex flex-col gap-1.5 text-[13px] leading-normal text-fg-muted max-sm:mt-6">
            <p className="text-fg-secondary">
              That&apos;s everything new worth your time.{next && ` Next check ${inAbout(next, now)}.`}
            </p>
            <p>Not now puts a role aside without changing what Narrow has learned. Not for me tells Narrow it doesn&apos;t fit.</p>
          </div>
        </>
      )}
    </div>
  );
}
