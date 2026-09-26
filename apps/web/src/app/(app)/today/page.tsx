import type { Metadata } from "next";
import { redirect } from "next/navigation";

import { putAside, recordFeedback } from "@/app/actions";
import { CaughtUp } from "@/components/caught-up";
import { FeedSummary } from "@/components/feed-summary";
import { OpportunityCard } from "@/components/opportunity-card";
import { RefreshControls } from "@/components/refresh-on-focus";
import { Notice } from "@/components/ui";
import { api, loadOrNoProfile } from "@/lib/api";

export const metadata: Metadata = { title: "Today" };

/** The primary screen: a few new recommendations, then nothing. */
export default async function TodayPage() {
  const feed = await loadOrNoProfile(() => api.feed(5));
  if (feed === "no_profile") redirect("/welcome");
  const now = new Date(feed.generated_at);
  const date = now.toLocaleDateString("en", { weekday: "long", day: "numeric", month: "long" });
  return (
    <div>
      <header className="mb-6 flex flex-wrap items-end justify-between gap-3">
        <div>
          <h1 className="font-serif text-3xl leading-tight tracking-tight sm:text-4xl">Today</h1>
          <p className="mt-1 text-muted">{date}</p>
        </div>
        <RefreshControls />
      </header>
      <div className="mb-6 space-y-3">
        <FeedSummary summary={feed.summary} />
        {!feed.learning.has_preferences && (
          <Notice title="Tell JobHunt what you want">
            Recommendations get sharper with a sentence about the roles, teams and pay you&apos;re after.{" "}
            <a href="/preferences" className="underline">
              Add preferences
            </a>
            .
          </Notice>
        )}
      </div>
      {feed.caught_up ? (
        <CaughtUp feed={feed} now={now} />
      ) : (
        <ol className="space-y-5" aria-label="Recommendations">
          {feed.items.map((item) => (
            <li key={item.id}>
              <OpportunityCard item={item} actions={{ feedback: recordFeedback, putAside }} now={now} />
            </li>
          ))}
        </ol>
      )}
      {!feed.caught_up && (
        <p className="mt-8 text-center text-sm text-muted">
          That&apos;s everything new worth your time. Come back later — JobHunt keeps looking.
        </p>
      )}
    </div>
  );
}
