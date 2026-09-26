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

  return (
    <Button variant="quiet" onClick={() => startTransition(() => router.refresh())} disabled={pending}>
      {pending ? "Checking…" : "Check again"}
    </Button>
  );
}
