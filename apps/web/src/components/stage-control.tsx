"use client";

import { useState, useTransition } from "react";

import type { ActionResult, FeedbackKind } from "@/app/actions";
import type { FeedbackResult, PipelineStage } from "@/lib/api-types";

import { sessionExpired } from "@/lib/navigation";

import { RejectDialog } from "./reject-dialog";
import { Button } from "./ui";

type Move = { action: FeedbackKind; label: string; asksWhy?: boolean; primary?: boolean };

/** The next steps from each stage, as the backend's feedback actions. */
export const MOVES: Record<string, Move[]> = {
  saved: [
    { action: "applied", label: "I applied", primary: true },
    { action: "unsave", label: "Remove" },
    { action: "reject", label: "Not for me", asksWhy: true },
  ],
  applied: [
    { action: "interview", label: "Interviewing", primary: true },
    { action: "reject", label: "Withdrew or turned down", asksWhy: true },
  ],
  interviewing: [
    { action: "offer", label: "Got an offer", primary: true },
    { action: "reject", label: "Withdrew or turned down", asksWhy: true },
  ],
  offer: [{ action: "reject", label: "Declined", asksWhy: true }],
  rejected: [{ action: "save", label: "Save it again" }],
};

export function StageControl({
  id,
  title,
  company,
  stage,
  change,
}: {
  id: string;
  title: string;
  company: string;
  stage: PipelineStage;
  change: (id: string, action: FeedbackKind, reason?: string) => Promise<ActionResult<FeedbackResult>>;
}) {
  const [pending, startTransition] = useTransition();
  const [error, setError] = useState<string | null>(null);
  const [asking, setAsking] = useState(false);
  const moves = MOVES[stage] ?? [];

  const run = (action: FeedbackKind, reason?: string) => {
    setError(null);
    startTransition(async () => {
      const result = await change(id, action, reason);
      if (!result.ok) {
        if (result.code === "unauthenticated") sessionExpired();
        else setError(`${result.title}. ${result.message}`);
      }
    });
  };

  return (
    <div>
      <div className="flex flex-wrap gap-2" aria-busy={pending}>
        {moves.map((m) => (
          <Button
            key={m.action}
            variant={m.primary ? "primary" : "quiet"}
            disabled={pending}
            onClick={() => (m.asksWhy ? setAsking(true) : run(m.action))}
          >
            {m.label}
            <span className="sr-only"> — {title}</span>
          </Button>
        ))}
      </div>
      {error && (
        <p role="alert" className="mt-1 text-sm text-negative">
          {error}
        </p>
      )}
      <RejectDialog
        open={asking}
        title={title}
        company={company}
        onCancel={() => setAsking(false)}
        onConfirm={(reason) => {
          setAsking(false);
          run("reject", reason);
        }}
      />
    </div>
  );
}
