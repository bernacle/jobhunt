"use client";

import { useRouter } from "next/navigation";
import { useEffect, useState, useTransition } from "react";

import { StateMessage } from "@/components/summary";
import { Button } from "@/components/ui";
import { describeError } from "@/lib/errors";

/**
 * A page that failed to load. The only action is to try again: nothing is
 * queued or kept in the browser, and nothing needs to be, because every
 * decision already made was confirmed by the API before it was shown as
 * done. Server errors reach the browser without their details (Next.js
 * strips them in production); the digest links the report to the log.
 */
export default function PageError({ error, reset }: { error: Error & { digest?: string }; reset: () => void }) {
  const router = useRouter();
  const [pending, startTransition] = useTransition();
  // Fetch the page's data again (a server render), then leave the error.
  const retry = () =>
    startTransition(() => {
      router.refresh();
      reset();
    });
  // In development the message says why; in production it's stripped, so
  // ask whether the API answers at all.
  const [unreachable, setUnreachable] = useState(() => /could not be reached|fetch failed|cloud_unavailable/i.test(error.message));
  useEffect(() => {
    console.error(error);
    let current = true;
    fetch("/api/status", { cache: "no-store" })
      .then((r) => r.json() as Promise<{ reachable: boolean }>)
      .then((status) => current && !status.reachable && setUnreachable(true))
      .catch(() => current && setUnreachable(true));
    return () => {
      current = false;
    };
  }, [error]);
  const described = describeError(unreachable ? "cloud_unavailable" : undefined);
  return (
    <div className="border-t border-line-subtle pt-6">
      <StateMessage
        kind="error"
        title={described.title}
        headingLevel={1}
        id="page-error"
        action={
          <Button onClick={retry} loading={pending} disabled={pending} className="max-sm:h-11">
            Try again
          </Button>
        }
        detail={error.digest && <p className="font-mono text-mono-s text-fg-muted">Reference {error.digest}</p>}
      >
        {described.message}
      </StateMessage>
    </div>
  );
}
