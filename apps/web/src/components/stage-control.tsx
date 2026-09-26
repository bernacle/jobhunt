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
  const [running, setRunning] = useState<FeedbackKind | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [asking, setAsking] = useState<Move | null>(null);
  const moves = MOVES[stage] ?? [];

  const run = (action: FeedbackKind, reason?: string) => {
    setError(null);
    setRunning(action);
    startTransition(async () => {
      const result = await change(id, action, reason);
      if (!result.ok) {
        if (result.code === "unauthenticated") sessionExpired();
        else setError(`${result.title}. ${result.message}`);
      }
      setRunning(null);
    });
  };

  return (
    <div>
      <div className="flex flex-wrap gap-1.5 lg:justify-end" aria-busy={pending}>
        {moves.map((m) => (
          <Button
            key={m.action}
            size="sm"
            variant={m.primary ? "secondary" : "ghost"}
            className="max-sm:h-11"
            disabled={pending}
            loading={pending && running === m.action}
            onClick={() => (m.asksWhy ? setAsking(m) : run(m.action))}
          >
            {m.label}
            <span className="sr-only"> — {title}</span>
          </Button>
        ))}
      </div>
      {error && (
        <p role="alert" className="mt-1 text-[13px] text-danger">
          {error}
        </p>
      )}
      <RejectDialog
        open={asking !== null}
        title={title}
        company={company}
        heading={asking?.label ?? "Not for me"}
        confirmLabel={asking?.label ?? "Not for me"}
        notNowHint={false}
        description={
          <>
            <span className="text-fg-body">
              {title} · {company}
            </span>
            . What happened? It&apos;s kept as you wrote it, and helps Narrow rank what comes next.
          </>
        }
        onCancel={() => setAsking(null)}
        onConfirm={(reason) => {
          setAsking(null);
          run("reject", reason);
        }}
      />
    </div>
  );
}
