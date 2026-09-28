"use client";

import Link from "next/link";
import { useState, useTransition } from "react";

import type { ActionResult, FeedbackKind } from "@/app/actions";
import type { FeedbackResult } from "@/lib/api-types";

import { sessionExpired } from "@/lib/navigation";

import { RejectDialog } from "./reject-dialog";
import { Button, textLinkClass } from "./ui";

export interface FeedbackActionsProps {
  id: string;
  title: string;
  company: string;
  /** The server actions (injected so the component stays testable). */
  actions: {
    feedback: (id: string, action: FeedbackKind, reason?: string) => Promise<ActionResult<FeedbackResult>>;
    putAside: (id: string) => Promise<ActionResult<FeedbackResult>>;
  };
  /** Called with what happened, once the API confirmed it. */
  onDone?: (outcome: Outcome) => void;
  /** Hide "Not now" (it only means something on Today). */
  canPutAside?: boolean;
  /**
   * `lead`: Today's lead (full-size buttons). `peer`: the denser peers.
   * `detail`: the opportunity page's action bar. On phones every variant
   * becomes a 2×2 grid of 44px targets.
   */
  variant?: "lead" | "peer" | "detail";
}

export type Outcome = { kind: "saved" | "applied" | "aside" | "rejected"; result: FeedbackResult };

type Pending = Outcome["kind"] | null;

const PENDING_LABEL: Record<Exclude<Pending, null>, string> = {
  saved: "Saving…",
  applied: "Marking as applied…",
  aside: "Putting it aside…",
  rejected: "Recording…",
};

/**
 * Save, I applied, Not for me (with an optional reason) and Not now. "Not
 * for me" is negative feedback that may shape what Narrow learns; "Not
 * now" only puts the role aside and teaches nothing. Nothing is assumed
 * done until the API confirms it; if it refuses, the buttons come back
 * and the reason is said.
 */
export function FeedbackActions({ id, title, company, actions, onDone, canPutAside = true, variant = "lead" }: FeedbackActionsProps) {
  const [pending, setPending] = useState<Pending>(null);
  const [error, setError] = useState<string | null>(null);
  const [rejecting, setRejecting] = useState(false);
  const [, startTransition] = useTransition();

  const run = (kind: Exclude<Pending, null>, call: () => Promise<ActionResult<FeedbackResult>>) => {
    setError(null);
    setPending(kind);
    startTransition(async () => {
      const result = await call();
      if (result.ok) {
        onDone?.({ kind, result: result.data });
      } else if (result.code === "unauthenticated") {
        sessionExpired();
      } else {
        setPending(null);
        setError(`${result.title}. ${result.message}`);
      }
    });
  };

  const busy = pending !== null;
  const size = variant === "lead" ? "md" : "sm";
  // Phones: 44px targets. The lead and the page's bar are a 2×2 grid,
  // primary bottom right; a peer keeps its four on one row.
  const peer = variant === "peer";
  const touch = peer ? "max-sm:h-11 max-sm:text-[13px]" : "max-sm:h-11 max-sm:w-full max-sm:px-3 max-sm:text-[13px]";
  // Peers are narrow: the quiet actions keep less padding.
  const quietClass = `${touch} ${peer ? "px-2!" : ""}`;
  // Quiet actions first, then the secondary, then one primary.
  return (
    <div className={variant === "detail" ? "" : "max-sm:w-full"}>
      <div
        className={`flex flex-wrap items-center gap-x-3 gap-y-2 ${peer ? "justify-between max-sm:-mx-2 max-sm:gap-x-1" : "justify-end max-sm:grid max-sm:grid-cols-2 max-sm:gap-2"}`}
        aria-busy={busy}
      >
        <div className={`flex flex-wrap gap-1 ${peer ? "" : "max-sm:contents"}`}>
          {canPutAside && (
            <Button
              variant="ghost"
              size={size}
              className={quietClass}
              disabled={busy}
              loading={pending === "aside"}
              onClick={() => run("aside", () => actions.putAside(id))}
              title="Puts it aside. Doesn't change what Narrow has learned."
            >
              Not now
            </Button>
          )}
          <Button
            variant="ghost"
            size={size}
            className={quietClass}
            disabled={busy}
            loading={pending === "rejected"}
            onClick={() => setRejecting(true)}
            title="Removes it and tells Narrow what doesn't fit."
          >
            Not for me
          </Button>
        </div>
        <div className={`flex flex-wrap gap-2 ${peer ? "max-sm:gap-1" : "max-sm:contents"}`}>
          <Button
            variant={variant === "peer" ? "ghost" : "secondary"}
            size={size}
            className={quietClass}
            disabled={busy}
            loading={pending === "applied"}
            onClick={() => run("applied", () => actions.feedback(id, "applied"))}
          >
            I applied
          </Button>
          <Button
            variant={variant === "peer" ? "secondary" : "primary"}
            size={size}
            className={touch}
            disabled={busy}
            loading={pending === "saved"}
            onClick={() => run("saved", () => actions.feedback(id, "save"))}
          >
            Save
          </Button>
        </div>
      </div>
      <p aria-live="polite" className="sr-only">
        {pending ? PENDING_LABEL[pending] : ""}
      </p>
      {error && (
        <p role="alert" className="mt-2 text-[13px] text-danger">
          {error}
        </p>
      )}
      <RejectDialog
        open={rejecting}
        title={title}
        company={company}
        onCancel={() => setRejecting(false)}
        onConfirm={(reason) => {
          setRejecting(false);
          run("rejected", () => actions.feedback(id, "reject", reason));
        }}
      />
    </div>
  );
}

/** What an opportunity says once an action went through. */
export function OutcomeLine({ outcome }: { outcome: Outcome }) {
  const learned = outcome.result.interpretation?.read_as ?? [];
  switch (outcome.kind) {
    case "saved":
      return (
        <p>
          Saved. It&apos;s in{" "}
          <Link href="/applications" className={textLinkClass}>
            Applications
          </Link>
          .
        </p>
      );
    case "applied":
      return (
        <p>
          Marked as applied. Track it in{" "}
          <Link href="/applications" className={textLinkClass}>
            Applications
          </Link>
          .
        </p>
      );
    case "aside":
      return <p>Put aside. It comes back only if it changes in a way that matters. Nothing was learned from it.</p>;
    case "rejected":
      return (
        <p>
          Won&apos;t be recommended again.
          {learned.length > 0 && <> Narrow read your reason as: {learned.join("; ")}.</>}
          {outcome.result.interpretation && !outcome.result.interpretation.understood && <> Your reason is kept as written.</>}
        </p>
      );
  }
}
