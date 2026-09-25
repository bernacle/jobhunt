"use client";

import Link from "next/link";
import { useState, useTransition } from "react";

import type { ActionResult, FeedbackKind } from "@/app/actions";
import type { FeedbackResult } from "@/lib/api-types";

import { sessionExpired } from "@/lib/navigation";

import { RejectDialog } from "./reject-dialog";
import { Button } from "./ui";

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
}

export type Outcome = { kind: "saved" | "applied" | "aside" | "rejected"; result: FeedbackResult };

type Pending = "saved" | "applied" | "aside" | "rejected" | null;

const PENDING_LABEL: Record<Exclude<Pending, null>, string> = {
  saved: "Saving…",
  applied: "Marking as applied…",
  aside: "Putting it aside…",
  rejected: "Recording…",
};

/**
 * Save, Not for me (with an optional reason), Applied and Not now. The
 * card reacts at once; if the API refuses, it goes back to how it was and
 * says why. Nothing is assumed done until the API confirms it.
 */
export function FeedbackActions({ id, title, company, actions, onDone, canPutAside = true }: FeedbackActionsProps) {
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
  return (
    <div>
      <div className="flex flex-wrap items-center gap-2" aria-busy={busy}>
        <Button variant="primary" disabled={busy} onClick={() => run("saved", () => actions.feedback(id, "save"))}>
          Save
        </Button>
        <Button variant="secondary" disabled={busy} onClick={() => setRejecting(true)}>
          Not for me
        </Button>
        <Button variant="quiet" disabled={busy} onClick={() => run("applied", () => actions.feedback(id, "applied"))}>
          I applied
        </Button>
        {canPutAside && (
          <Button
            variant="quiet"
            disabled={busy}
            onClick={() => run("aside", () => actions.putAside(id))}
            title="Hide it for now. JobHunt learns nothing from this."
          >
            Not now
          </Button>
        )}
      </div>
      <p aria-live="polite" className="mt-2 min-h-5 text-sm text-muted">
        {pending ? PENDING_LABEL[pending] : ""}
      </p>
      {error && (
        <p role="alert" className="text-sm text-negative">
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

/** What the card says once an action went through. */
export function OutcomeLine({ outcome }: { outcome: Outcome }) {
  const learned = outcome.result.interpretation?.read_as ?? [];
  switch (outcome.kind) {
    case "saved":
      return (
        <p>
          Saved. It&apos;s in{" "}
          <Link href="/applications" className="underline">
            Applications
          </Link>
          .
        </p>
      );
    case "applied":
      return (
        <p>
          Marked as applied. Track it in{" "}
          <Link href="/applications" className="underline">
            Applications
          </Link>
          .
        </p>
      );
    case "aside":
      return <p>Put aside. It comes back only if it changes in a way that matters.</p>;
    case "rejected":
      return (
        <p>
          Won&apos;t be recommended again.
          {learned.length > 0 && <> JobHunt read your reason as: {learned.join("; ")}.</>}
          {outcome.result.interpretation && !outcome.result.interpretation.understood && (
            <> Your reason is kept as written.</>
          )}
        </p>
      );
  }
}
