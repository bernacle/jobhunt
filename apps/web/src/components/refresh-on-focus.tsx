"use client";

import { useRouter } from "next/navigation";
import { useEffect, useRef, useTransition } from "react";

import { Button } from "./ui";

/**
 * Keeps Today current without real-time machinery: refreshes when the
 * person comes back to the tab (if the page is older than `staleAfter`),
 * every `every` while it stays visible, and on demand. No polling while
 * the tab is hidden.
 */
export function RefreshControls({
  staleAfterMs = 5 * 60_000,
  everyMs = 15 * 60_000,
}: {
  staleAfterMs?: number;
  everyMs?: number;
}) {
  const router = useRouter();
  // When the page last rendered with fresh data (set after each render).
  const loadedAt = useRef(0);
  const [pending, startTransition] = useTransition();

  useEffect(() => {
    loadedAt.current = Date.now();
  });

  useEffect(() => {
    const refresh = () => startTransition(() => router.refresh());
    const onVisible = () => {
      if (document.visibilityState === "visible" && Date.now() - loadedAt.current > staleAfterMs) refresh();
    };
    document.addEventListener("visibilitychange", onVisible);
    const timer = window.setInterval(() => {
      if (document.visibilityState === "visible") refresh();
    }, everyMs);
    return () => {
      document.removeEventListener("visibilitychange", onVisible);
      window.clearInterval(timer);
    };
  }, [router, staleAfterMs, everyMs]);

  return <RefreshButton pending={pending} onRefresh={() => startTransition(() => router.refresh())} />;
}

/**
 * "Check again", and while a refresh runs, a quiet line saying so. The list
 * on screen stays, and stays usable: the refresh is a transition, so the
 * page never falls back to its loading skeleton. Nothing is counted or
 * estimated here; there is no progress to show, only that it's checking.
 */
export function RefreshButton({ pending, onRefresh }: { pending: boolean; onRefresh: () => void }) {
  return (
    <>
      <Button variant="ghost" size="sm" className="-ml-3 max-sm:h-11" onClick={onRefresh} disabled={pending} loading={pending}>
        {pending ? "Checking" : "Check again"}
      </Button>
      <span role="status" className="text-[13px] text-fg-muted empty:hidden">
        {pending ? "Checking for new opportunities…" : ""}
      </span>
    </>
  );
}
