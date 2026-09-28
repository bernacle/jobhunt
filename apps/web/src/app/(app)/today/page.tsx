import type { Metadata } from "next";
import { redirect } from "next/navigation";

import { putAside, recordFeedback } from "@/app/actions";
import { CaughtUp } from "@/components/caught-up";
import { DiscoveryStatus, FeedSummary } from "@/components/feed-summary";
import { TodayFeed } from "@/components/opportunity";
import { RefreshControls } from "@/components/refresh-on-focus";
import { Notice, PageHeader, textLinkClass } from "@/components/ui";
import { api, loadOrNoProfile } from "@/lib/api";
import { inAbout } from "@/lib/format";

export const metadata: Metadata = { title: "Today" };

/**
 * The primary screen, and a finite one: one lead, a few peers, then an
 * explicit end. Nothing is added to make it look full.
 */
export default async function TodayPage() {
  const [feed, profile] = await Promise.all([loadOrNoProfile(() => api.feed(5)), loadOrNoProfile(() => api.profile())]);
  if (feed === "no_profile") redirect("/welcome");
  // Preferences read from the person's words with an open question: not
  // used the way they meant until answered.
  const unresolved = profile === "no_profile" ? 0 : profile.preferences.filter((p) => p.active && p.clarify).length;
  const now = new Date(feed.generated_at);
  const date = now.toLocaleDateString("en-GB", { weekday: "long", day: "numeric", month: "long" });
  const actions = { feedback: recordFeedback, putAside };
  const lead = feed.items[0];
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
        <FeedSummary summary={feed.summary} />
        <DiscoveryStatus discovery={feed.discovery} now={now} />
        <RefreshControls />
      </div>
      {unresolved > 0 && (
        <div className="mb-8">
          <Notice title={unresolved === 1 ? "A preference needs your answer" : `${unresolved} preferences need your answer`}>
            Until then {unresolved === 1 ? "it isn't" : "they aren't"} used the way you meant.{" "}
            <a href="/preferences" className={textLinkClass}>
              Answer in Preferences
            </a>
          </Notice>
        </div>
      )}
      {!feed.learning.has_preferences && (
        <div className="mb-8">
          <Notice title="Tell Narrow what you want">
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
          <TodayFeed items={feed.items} actions={actions} now={now} />
          <p className="mt-9 text-[13px] leading-normal text-fg-muted max-sm:mt-6">
            That&apos;s everything new worth your time.{next && ` Next check ${inAbout(next, now)}.`}
          </p>
        </>
      )}
    </div>
  );
}
