"use client";

import { useId, useState, useTransition } from "react";

import type { ActionResult } from "@/app/actions";
import type { ClaimDecisionResult, UnresolvedClaim } from "@/lib/api-types";
import { sentence } from "@/lib/format";

import { Button } from "./ui";

const PROVENANCE: Record<string, string> = {
  extracted: "Read from your resume",
  inferred: "Concluded by JobHunt",
  user_entered: "Entered by you",
};

type Decide = (id: string, decision: "confirm" | "reject" | "reset", note?: string) => Promise<ActionResult<ClaimDecisionResult>>;

function ClaimItem({ claim, decide, onDecided }: { claim: UnresolvedClaim; decide: Decide; onDecided: (text: string) => void }) {
  const [pending, startTransition] = useTransition();
  const [error, setError] = useState<string | null>(null);
  const [rejecting, setRejecting] = useState(false);
  const [note, setNote] = useState("");
  const noteId = useId();

  const run = (decision: "confirm" | "reject") =>
    startTransition(async () => {
      setError(null);
      const r = await decide(claim.id, decision, decision === "reject" ? note : undefined);
      if (r.ok) onDecided(decision === "confirm" ? "Confirmed: it can be used as evidence." : "Rejected: it will never be used.");
      else setError(`${r.title}. ${r.message}`);
    });

  return (
    <li className="rounded-xl border border-line bg-surface p-4">
      <p className="text-[0.95rem] font-medium">{claim.text}</p>
      <p className="mt-1 text-sm text-muted">
        {PROVENANCE[claim.provenance ?? ""] ?? claim.provenance}
        {claim.about && <> · about {claim.about}</>}
        {claim.confidence && claim.provenance === "extracted" && <> · {claim.confidence} confidence</>}
      </p>
      {claim.snippet && (
        <blockquote className="mt-2 border-l-2 border-line-strong pl-3 text-sm">
          <q>{claim.snippet}</q>
          {(claim.section || claim.document) && (
            <footer className="mt-0.5 text-xs text-muted">
              {[claim.section, claim.document].filter(Boolean).join(" · ")}
            </footer>
          )}
        </blockquote>
      )}
      {claim.basis && <p className="mt-2 text-sm text-muted">Based on: {claim.basis}</p>}
      <p className="mt-2 text-sm">
        <span className="text-muted">Why it needs review:</span> {sentence(claim.why)}.
      </p>
      {rejecting && (
        <div className="mt-3">
          <label htmlFor={noteId} className="block text-sm text-muted">
            Why is it wrong? <span className="text-xs">(optional)</span>
          </label>
          <input
            id={noteId}
            value={note}
            onChange={(e) => setNote(e.target.value)}
            maxLength={300}
            className="mt-1 w-full rounded-md border border-line-strong bg-canvas px-2 py-1.5 text-sm"
          />
        </div>
      )}
      <div className="mt-3 flex flex-wrap gap-2" aria-busy={pending}>
        {rejecting ? (
          <>
            <Button variant="danger" disabled={pending} onClick={() => run("reject")}>
              Reject claim
            </Button>
            <Button variant="quiet" disabled={pending} onClick={() => setRejecting(false)}>
              Cancel
            </Button>
          </>
        ) : (
          <>
            <Button variant="primary" disabled={pending} onClick={() => run("confirm")}>
              Confirm<span className="sr-only">: {claim.text}</span>
            </Button>
            <Button disabled={pending} onClick={() => setRejecting(true)}>
              Reject<span className="sr-only">: {claim.text}</span>
            </Button>
          </>
        )}
      </div>
      {error && (
        <p role="alert" className="mt-2 text-sm text-negative">
          {error}
        </p>
      )}
    </li>
  );
}

/**
 * Claims JobHunt could not settle on its own. Only confirmed claims (or
 * ones quoted from the current resume) are used when helping with an
 * application; rejected ones never are, even after a re-import.
 */
export function ClaimReview({ claims, total, decide }: { claims: UnresolvedClaim[]; total: number; decide: Decide }) {
  const [done, setDone] = useState<Record<string, string>>({});
  const [all, setAll] = useState(false);
  const pending = claims.filter((c) => !done[c.id]);
  const open = all ? pending : pending.slice(0, 5);
  const decidedCount = Object.keys(done).length;
  return (
    <div>
      <p aria-live="polite" className="text-sm text-muted">
        {total - decidedCount > 0
          ? `${total - decidedCount} ${total - decidedCount === 1 ? "claim needs" : "claims need"} your review.`
          : "Nothing left to review."}
      </p>
      <ul className="mt-3 space-y-3">
        {open.map((c) => (
          <ClaimItem key={c.id} claim={c} decide={decide} onDecided={(text) => setDone((d) => ({ ...d, [c.id]: text }))} />
        ))}
      </ul>
      {!all && pending.length > open.length && (
        <Button variant="quiet" className="mt-2" onClick={() => setAll(true)}>
          Show all {pending.length}
        </Button>
      )}
      {decidedCount > 0 && (
        <div role="status">
          <ul className="mt-3 space-y-1 text-sm text-muted">
            {claims
              .filter((c) => done[c.id])
              .map((c) => (
                <li key={c.id}>
                  “{c.text}” — {done[c.id]}
                </li>
              ))}
          </ul>
        </div>
      )}
    </div>
  );
}
