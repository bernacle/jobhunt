import type { Metadata } from "next";
import Link from "next/link";
import { redirect } from "next/navigation";

import { decideClaim } from "@/app/actions";
import { ClaimReview } from "@/components/claim-review";
import { StateMessage } from "@/components/summary";
import { LinkButton, PageHeader } from "@/components/ui";
import { api, loadOrNoProfile } from "@/lib/api";

export const metadata: Metadata = { title: "Review claims" };

/**
 * Claims Narrow read but isn't sure of, to confirm or reject: a task of
 * its own, one step from Profile. Nothing here is used until confirmed.
 */
export default async function ClaimReviewPage() {
  const claims = await loadOrNoProfile(() => api.claims());
  if (claims === "no_profile") redirect("/welcome");
  return (
    <div className="max-w-[640px]">
      <nav aria-label="Breadcrumb" className="mb-4">
        <ol className="flex items-center gap-2 text-ui-m text-fg-muted">
          <li>
            <Link href="/profile" className="text-fg-secondary hover:text-fg max-sm:inline-flex max-sm:min-h-11 max-sm:items-center">
              Profile
            </Link>
          </li>
          <li aria-hidden="true">/</li>
          <li aria-current="page" className="text-fg-body">
            Review
          </li>
        </ol>
      </nav>
      <PageHeader title="Review claims" />
      {claims.total > 0 ? (
        <section id="review" aria-label="Needs your review" className="-mt-4">
          <ClaimReview claims={claims.claims} total={claims.total} decide={decideClaim} />
        </section>
      ) : (
        <StateMessage
          kind="done"
          title="Nothing to review."
          action={
            <LinkButton href="/profile" className="max-sm:h-11">
              Back to Profile
            </LinkButton>
          }
        >
          Every claim Narrow read is settled.
        </StateMessage>
      )}
    </div>
  );
}
