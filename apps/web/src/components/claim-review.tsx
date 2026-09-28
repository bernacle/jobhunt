"use client";

import { useId, useState, useTransition } from "react";

import type { ActionResult } from "@/app/actions";
import type { ClaimDecisionResult, UnresolvedClaim } from "@/lib/api-types";
import { sentence } from "@/lib/format";

import { EvidenceFact, EvidenceQuote, EvidenceSection } from "./evidence";
import { EvidenceTrigger } from "./evidence-panel";
import { Button, inputClass, labelClass } from "./ui";

const PROVENANCE: Record<string, string> = {
  extracted: "Read from your resume",
  inferred: "Inferred by Narrow",
  user_entered: "Entered by you",
};

// The same, as a group's context line.
const FROM: Record<string, string> = {
  extracted: "read from your resume",
  inferred: "inferred by Narrow",
  user_entered: "entered by you",
};

type Decide = (id: string, decision: "confirm" | "reject" | "reset", note?: string) => Promise<ActionResult<ClaimDecisionResult>>;

/** The words behind a claim, where they are from, and why it needs review: on demand. */
function ClaimEvidence({ claim }: { claim: UnresolvedClaim }) {
  const where = [claim.document, claim.section].filter(Boolean).join(" · ");
  const provenance = PROVENANCE[claim.provenance ?? ""] ?? claim.provenance;
  return (
    <EvidenceTrigger
      label="See evidence"
      srLabel={`for ${claim.text}`}
      title={claim.text}
      subtitle={[claim.about, provenance].filter(Boolean).join(" · ")}
      conclusion={`${sentence(claim.why)}.`}
    >
      {claim.snippet && (
        <EvidenceSection label="From your resume">
          <EvidenceQuote source={where || undefined}>{claim.snippet}</EvidenceQuote>
        </EvidenceSection>
      )}
      <EvidenceSection label="How Narrow got it">
        {provenance && <EvidenceFact label="Source">{provenance}</EvidenceFact>}
        {claim.about && <EvidenceFact label="About">{claim.about}</EvidenceFact>}
        {claim.basis && <EvidenceFact label="Based on">{claim.basis}</EvidenceFact>}
        {claim.confidence && claim.provenance === "extracted" && <EvidenceFact label="Reading">{claim.confidence} confidence</EvidenceFact>}
      </EvidenceSection>
      <EvidenceSection label="What your answer does">
        <EvidenceFact label="Confirm">It can be used as evidence.</EvidenceFact>
        <EvidenceFact label="Reject">It&apos;s never used, even after a re-import.</EvidenceFact>
      </EvidenceSection>
    </EvidenceTrigger>
  );
}

/**
 * One claim to settle: the claim, then Confirm, Reject and See evidence on
 * the same line. Once decided it resolves in place.
 */
function ClaimReviewRow({ claim, decide, outcome, onDecided }: { claim: UnresolvedClaim; decide: Decide; outcome?: string; onDecided: (text: string) => void }) {
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

  const touch = "max-sm:h-11 max-sm:flex-1";
  return (
    <li className="border-t border-line-subtle py-2.5 max-sm:py-3.5">
      <div className="flex items-center justify-between gap-x-4 gap-y-2.5 max-sm:flex-col max-sm:items-stretch sm:min-h-9">
        <p className={`min-w-0 text-[14px] leading-[1.4] font-medium ${outcome?.startsWith("Rejected") ? "text-fg-muted" : "text-fg"}`}>{claim.text}</p>
        {outcome ? (
          <p role="status" className="shrink-0 text-[13px] text-fg-secondary">
            {outcome}
          </p>
        ) : (
          <div className="flex shrink-0 flex-wrap items-center gap-1.5 max-sm:gap-2" aria-busy={pending}>
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
                <span className="sm:ml-2 max-sm:basis-full">
                  <ClaimEvidence claim={claim} />
                </span>
              </>
            )}
          </div>
        )}
      </div>
      {rejecting && !outcome && (
        <div className="mt-2.5 max-w-[420px]">
          <label htmlFor={noteId} className={labelClass}>
            Why is it wrong? <span className="text-fg-muted">(optional)</span>
          </label>
          <input id={noteId} value={note} onChange={(e) => setNote(e.target.value)} maxLength={300} className={inputClass} />
        </div>
      )}
      {error && (
        <p role="alert" className="mt-2 text-[13px] text-danger">
          {error}
        </p>
      )}
    </li>
  );
}

function groupKey(c: UnresolvedClaim): string {
  return `${c.about ?? ""}\u0000${c.provenance ?? ""}`;
}

/**
 * Claims Narrow could not settle on its own, grouped by what they are
 * about and where they came from, so the context is said once. Nothing
 * here is used until confirmed; rejected claims are never used, even after
 * a re-import.
 */
export function ClaimReview({ claims, total, decide }: { claims: UnresolvedClaim[]; total: number; decide: Decide }) {
  const [done, setDone] = useState<Record<string, string>>({});
  const [all, setAll] = useState(false);
  const decidedCount = Object.keys(done).length;
  // Decided rows stay where they were; more open ones take their turn.
  const visible = all ? claims : claims.slice(0, 5 + decidedCount);
  const hidden = claims.length - visible.length;
  const left = total - decidedCount;
  const groups: { key: string; about: string; source: string; items: UnresolvedClaim[] }[] = [];
  for (const c of visible) {
    const key = groupKey(c);
    let group = groups.find((g) => g.key === key);
    if (!group) {
      group = { key, about: c.about ?? "Your profile in general", source: FROM[c.provenance ?? ""] ?? "", items: [] };
      groups.push(group);
    }
    group.items.push(c);
  }
  return (
    <div>
      <p aria-live="polite" className="font-mono text-mono-s text-fg-muted">
        {left > 0 ? `${left} ${left === 1 ? "claim needs" : "claims need"} your review.` : "Nothing left to review."}
      </p>
      <div className="mt-6 flex flex-col gap-8">
        {groups.map((g) => (
          <section key={g.key} aria-label={g.about}>
            <p className="mb-1 flex flex-wrap items-baseline gap-x-2">
              <span className="text-[15px] leading-[1.4] font-semibold text-fg">{g.about}</span>
              {g.source && <span className="text-[13px] text-fg-muted">{g.source}</span>}
            </p>
            <ul role="list" className="border-b border-line-subtle">
              {g.items.map((c) => (
                <ClaimReviewRow key={c.id} claim={c} decide={decide} outcome={done[c.id]} onDecided={(text) => setDone((d) => ({ ...d, [c.id]: text }))} />
              ))}
            </ul>
          </section>
        ))}
      </div>
      {hidden > 0 && (
        <Button variant="ghost" className="mt-4 -ml-3 max-sm:h-11" onClick={() => setAll(true)}>
          Show all {claims.length - decidedCount}
        </Button>
      )}
    </div>
  );
}
