"use client";

import { useId, useState, useTransition } from "react";

import type { ActionResult } from "@/app/actions";
import type { ClaimDecisionResult, UnresolvedClaim } from "@/lib/api-types";
import { sentence } from "@/lib/format";

import { Button, inputClass, labelClass } from "./ui";

const PROVENANCE: Record<string, string> = {
  extracted: "Read from your resume",
  inferred: "Inferred by Narrow",
  user_entered: "Entered by you",
};

type Decide = (id: string, decision: "confirm" | "reject" | "reset", note?: string) => Promise<ActionResult<ClaimDecisionResult>>;

function ClaimItem({ claim, decide, onDecided }: { claim: UnresolvedClaim; decide: Decide; onDecided: (text: string) => void }) {
  const [pending, startTransition] = useTransition();
  const [running, setRunning] = useState<"confirm" | "reject" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [rejecting, setRejecting] = useState(false);
  const [note, setNote] = useState("");
  const noteId = useId();

  const run = (decision: "confirm" | "reject") =>
    startTransition(async () => {
      setError(null);
      setRunning(decision);
      const r = await decide(claim.id, decision, decision === "reject" ? note : undefined);
      setRunning(null);
      if (r.ok) onDecided(decision === "confirm" ? "Confirmed: it can be used as evidence." : "Rejected: it will never be used.");
      else setError(`${r.title}. ${r.message}`);
    });

  const where = [claim.section, claim.document].filter(Boolean).join(" · ");
  const touch = "max-sm:h-11 max-sm:w-full";
  return (
    <li className="border-t border-line-subtle py-5 max-sm:py-[18px]">
      <p className="text-[15px] leading-[1.4] font-semibold text-fg">{claim.text}</p>
      {claim.snippet && (
        <blockquote className="mt-2.5 rounded-md border border-line-subtle bg-inset px-3.5 py-3 text-[14px] leading-relaxed text-fg-body max-sm:px-3">
          <q>{claim.snippet}</q>
        </blockquote>
      )}
      <p className="mt-2 flex flex-wrap gap-x-1.5 font-mono text-mono-s text-fg-muted">
        <span>{PROVENANCE[claim.provenance ?? ""] ?? claim.provenance}</span>
        {claim.about && <span>· about {claim.about}</span>}
        {where && (
          <>
            <span aria-hidden="true">·</span>
            <span>{where}</span>
          </>
        )}
        {claim.confidence && claim.provenance === "extracted" && <span>· {claim.confidence} confidence</span>}
      </p>
      {claim.basis && <p className="mt-2 text-[13.5px] text-fg-secondary">Based on: {claim.basis}</p>}
      <p className="mt-2.5 flex gap-2.5 text-[13.5px] leading-normal text-fg-secondary">
        <span aria-hidden="true" className="mt-[0.55em] size-[5px] shrink-0 rounded-[1px] border border-fg-secondary" />
        <span>
          <span className="sr-only">Why it needs review: </span>
          {sentence(claim.why)}.
        </span>
      </p>
      {rejecting && (
        <div className="mt-3.5 max-w-[420px]">
          <label htmlFor={noteId} className={labelClass}>
            Why is it wrong? <span className="text-fg-muted">(optional)</span>
          </label>
          <input id={noteId} value={note} onChange={(e) => setNote(e.target.value)} maxLength={300} className={inputClass} />
        </div>
      )}
      <div className="mt-3.5 flex flex-wrap items-center gap-2 max-sm:grid max-sm:grid-cols-2" aria-busy={pending}>
        {rejecting ? (
          <>
            <Button size="sm" variant="destructive" className={touch} disabled={pending} loading={running === "reject"} onClick={() => run("reject")}>
              Reject claim
            </Button>
            <Button size="sm" variant="ghost" className={touch} disabled={pending} onClick={() => setRejecting(false)}>
              Cancel
            </Button>
          </>
        ) : (
          <>
            <Button size="sm" variant="secondary" className={touch} disabled={pending} loading={running === "confirm"} onClick={() => run("confirm")}>
              Confirm<span className="sr-only">: {claim.text}</span>
            </Button>
            <Button size="sm" variant="ghost" className={touch} disabled={pending} onClick={() => setRejecting(true)}>
              Reject<span className="sr-only">: {claim.text}</span>
            </Button>
          </>
        )}
      </div>
      {error && (
        <p role="alert" className="mt-2 text-[13px] text-danger">
          {error}
        </p>
      )}
    </li>
  );
}

/**
 * Claims Narrow could not settle on its own, each with the words behind
 * it and why it needs review. Nothing here is used until confirmed;
 * rejected claims are never used, even after a re-import.
 */
export function ClaimReview({ claims, total, decide }: { claims: UnresolvedClaim[]; total: number; decide: Decide }) {
  const [done, setDone] = useState<Record<string, string>>({});
  const [all, setAll] = useState(false);
  const pending = claims.filter((c) => !done[c.id]);
  const open = all ? pending : pending.slice(0, 5);
  const decidedCount = Object.keys(done).length;
  const left = total - decidedCount;
  return (
    <div>
      <p aria-live="polite" className="font-mono text-mono-s text-fg-muted">
        {left > 0 ? `${left} ${left === 1 ? "claim needs" : "claims need"} your review.` : "Nothing left to review."}
      </p>
      <ul role="list" className="mt-4">
        {open.map((c) => (
          <ClaimItem key={c.id} claim={c} decide={decide} onDecided={(text) => setDone((d) => ({ ...d, [c.id]: text }))} />
        ))}
      </ul>
      {!all && pending.length > open.length && (
        <Button variant="ghost" className="-ml-3 max-sm:h-11" onClick={() => setAll(true)}>
          Show all {pending.length}
        </Button>
      )}
      {decidedCount > 0 && (
        <div role="status" className="border-t border-line-subtle pt-3">
          <ul className="space-y-1 text-[13px] text-fg-secondary">
            {claims
              .filter((c) => done[c.id])
              .map((c) => (
                <li key={c.id}>
                  <span className="text-fg-body">“{c.text}”</span> — {done[c.id]}
                </li>
              ))}
          </ul>
        </div>
      )}
    </div>
  );
}
