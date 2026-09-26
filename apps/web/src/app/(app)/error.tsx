"use client";

import { useRouter } from "next/navigation";
import { useEffect, useState, useTransition } from "react";

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
    <div role="alert" className="max-w-[560px] border-t border-line-subtle pt-6">
      <div className="flex items-center gap-2.5">
        <span aria-hidden="true" className="size-1.5 shrink-0 rounded-[1px] bg-info" />
        <h1 className="text-heading-m">{described.title}</h1>
      </div>
      <p className="mt-2.5 text-body-s text-pretty text-fg-secondary">{described.message}</p>
      <div className="mt-4 flex flex-wrap items-center gap-4">
        <Button onClick={retry} loading={pending} disabled={pending} className="max-sm:h-11">
          Try again
        </Button>
        {error.digest && <span className="font-mono text-mono-s text-fg-muted">Reference {error.digest}</span>}
      </div>
    </div>
  );
}
