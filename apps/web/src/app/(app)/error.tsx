"use client";

import { useEffect } from "react";

import { Button, Notice } from "@/components/ui";
import { describeError } from "@/lib/errors";

/**
 * A page that failed to load. Server errors reach the browser without
 * their details (Next.js strips them in production); the digest links the
 * browser's report to the server log.
 */
export default function PageError({ error, reset }: { error: Error & { digest?: string }; reset: () => void }) {
  useEffect(() => {
    console.error(error);
  }, [error]);
  const unreachable = /could not be reached|fetch failed|cloud_unavailable/i.test(error.message);
  const described = describeError(unreachable ? "cloud_unavailable" : undefined);
  return (
    <div className="space-y-4">
      <Notice tone="error" role="alert" title={described.title}>
        {described.message}
        {error.digest && <span className="mt-1 block text-xs">Reference: {error.digest}</span>}
      </Notice>
      <Button onClick={reset}>Try again</Button>
    </div>
  );
}
