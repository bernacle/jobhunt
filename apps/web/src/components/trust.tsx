import type { ReactNode } from "react";

import type { CompensationView, EligibilityBrief, FitTier, VerificationBrief } from "@/lib/api-types";
import {
  TIER_LABEL,
  type VerificationMark,
  compensationLine,
  eligibilityFact,
  placeLine,
  unscopedRemote,
  verificationLine,
  verificationMark,
  verificationShort,
} from "@/lib/format";

/*
 * Verification and uncertainty are part of the brand. Facts carry a source
 * and a time. A verified fact gets the mint check; an inferred or
 * unresolved one gets less ink and a dotted underline (never a warning
 * colour); an unknown is stated plainly in muted ink; only a real caution
 * gets sand. Meaning never rests on colour alone: shapes and words differ.
 */

/** The coarse fit tier, in words. Never a percentage or a score. */
export function TierLabel({ tier, className = "" }: { tier: FitTier; className?: string }) {
  return (
    <span className={`text-label whitespace-nowrap ${tier === "strong_fit" ? "text-fg" : "text-fg-secondary"} ${className}`}>
      {TIER_LABEL[tier]}
    </span>
  );
}

/** The company's initial, as a quiet tile (never a logo we don't have). */
export function CompanyTile({ name }: { name: string }) {
  return (
    <span
      aria-hidden="true"
      className="grid size-7 shrink-0 place-items-center rounded-sm border border-line-subtle bg-overlay-hover text-[12px] font-semibold text-fg-secondary"
    >
      {(name.trim()[0] ?? "?").toUpperCase()}
    </span>
  );
}

/** First-party verified: the one place the mint check appears. */
export function VerifiedCheck({ aging = false }: { aging?: boolean }) {
  return (
    <span className={aging ? "text-verified-aging" : "text-verified"}>
      <span aria-hidden="true">✓</span>
      <span className="sr-only"> verified</span>
    </span>
  );
}

/** The mark a verification earned: mint check, grey check, or a plain dot. */
export function VerificationGlyph({ mark }: { mark: VerificationMark }) {
  if (mark) return <VerifiedCheck aging={mark === "aging"} />;
  return (
    <span aria-hidden="true" className="text-fg-muted">
      ·
    </span>
  );
}

/** A value read or estimated, not confirmed: less ink, dotted underline. */
export function Inferred({ children }: { children: ReactNode }) {
  return <span className="nr-inferred">{children}</span>;
}

/**
 * How current the listing is, in mono: "✓ Verified 18 min ago on the
 * employer's job board". Aging checks lose the mint; stale, failed and
 * unverified listings lose the check.
 */
export function VerificationStamp({ verification, now, className = "" }: { verification: VerificationBrief; now?: Date; className?: string }) {
  const mark = verificationMark(verification);
  return (
    <p className={`font-mono text-mono-s text-fg-muted ${className}`}>
      {mark && (
        <>
          <VerifiedCheck aging={mark === "aging"} />{" "}
        </>
      )}
      {verificationLine(verification, now)}
    </p>
  );
}

/**
 * How current the listing is, briefly: what was checked and when, in
 * mono. `withSource` names where for a trusted verification; otherwise the
 * reason it isn't trusted is left to the "check first" line and the
 * evidence, so it isn't said twice.
 */
export function VerificationStatus({
  verification,
  now,
  withSource = false,
  className = "",
}: {
  verification: VerificationBrief;
  now?: Date;
  withSource?: boolean;
  className?: string;
}) {
  const mark = verificationMark(verification);
  const text = withSource && verification.state === "verified_active" && verification.trusted ? verificationLine(verification, now) : verificationShort(verification, now);
  return (
    <p className={`font-mono text-mono-s text-fg-muted ${className}`}>
      {mark && (
        <>
          <VerifiedCheck aging={mark === "aging"} />{" "}
        </>
      )}
      {text}
    </p>
  );
}

/**
 * Eligibility as a fact: only a pass reads as settled. `brief` leaves out
 * the headline of a pass (the evidence has it); a condition or an unclear
 * reading always says what it is.
 */
export function EligibilityFact({ eligibility, brief = false }: { eligibility: EligibilityBrief; brief?: boolean }) {
  const fact = eligibilityFact(eligibility);
  const detail = fact.detail && !(brief && fact.kind === "resolved") && <span className="text-fg-secondary"> · {fact.detail}</span>;
  switch (fact.kind) {
    case "resolved":
      return (
        <span>
          {fact.label}
          {detail}
        </span>
      );
    case "conditional":
    case "unclear":
      return (
        <span>
          <Inferred>{fact.label}</Inferred>
          {detail}
        </span>
      );
    case "ineligible":
      return (
        <span className="text-warning">
          {fact.label}
          {fact.detail && <> · {fact.detail}</>}
        </span>
      );
    default:
      return <span className="text-missing">{fact.label}</span>;
  }
}

/** Pay: verified at the source, as listed, or plainly not published. */
export function PayFact({ compensation }: { compensation: CompensationView }) {
  const pay = compensationLine(compensation);
  if (!pay.known) return <span className="text-missing">{pay.text}</span>;
  if (compensation.status === "published" && compensation.ranges.length > 0 && compensation.verified) {
    return (
      <span className="nr-tnum">
        {pay.text} <VerifiedCheck />
      </span>
    );
  }
  return <span className="nr-tnum text-fg-secondary">{pay.text}</span>;
}

/** Where: "Remote" without a scope stays visibly unresolved. */
export function PlaceFact({ locations, workplace }: { locations?: string[]; workplace?: string | null }) {
  if (unscopedRemote(locations, workplace)) return <Inferred>Remote · region not stated</Inferred>;
  const place = placeLine(locations, workplace);
  return place ? <span>{place}</span> : <span className="text-missing">Location not stated</span>;
}

/**
 * A row of short facts divided by hairlines. On phones the facts stack,
 * one per line, so none is cut off mid-sentence.
 */
export function FactRow({ items, className = "", size = "md" }: { items: ReactNode[]; className?: string; size?: "md" | "sm" }) {
  const list = items.filter(Boolean);
  if (list.length === 0) return null;
  return (
    <ul
      role="list"
      className={`flex flex-wrap items-baseline gap-y-1.5 text-fg nr-tnum max-sm:flex-col max-sm:gap-y-1 ${
        size === "md" ? "text-[14px]" : "text-[13.5px]"
      } ${className}`}
    >
      {list.map((item, i) => (
        <li key={i} className="flex items-baseline">
          {i > 0 && <span aria-hidden="true" className="mx-3.5 h-3.5 w-px self-center bg-fg/12 max-sm:hidden" />}
          {item}
        </li>
      ))}
    </ul>
  );
}

export type ConsiderationKind = "reason" | "caution" | "unclear" | "unresolved" | "missing";

const MARKER: Record<ConsiderationKind, string> = {
  reason: "bg-fg-secondary",
  caution: "bg-warning",
  unclear: "border border-fg-secondary",
  unresolved: "border border-fg-secondary",
  missing: "border border-fg-muted",
};

// Read before the line by screen readers; sighted readers get the shape.
// An unresolved requirement already says "Unresolved:" in its words.
const SPOKEN: Record<ConsiderationKind, string> = {
  reason: "Reason: ",
  caution: "Caution: ",
  unclear: "Unclear: ",
  unresolved: "",
  missing: "Not stated: ",
};

const SIZE = { md: "text-[14px]", sm: "text-[13.5px]", lg: "text-body-m" };

/**
 * One line of an opportunity's explanation. A reason is a small solid
 * square; a caution from a source a sand square; an unclear reading or an
 * unresolved requirement a hollow square in secondary ink; something the
 * posting doesn't say a hollow square in muted ink.
 */
export function Consideration({
  kind,
  children,
  size = "md",
  className = "",
}: {
  kind: ConsiderationKind;
  children: ReactNode;
  size?: "md" | "sm" | "lg";
  className?: string;
}) {
  return (
    <li className={`flex gap-2.5 leading-normal ${SIZE[size]} ${kind === "missing" || kind === "unresolved" ? "text-fg-secondary" : "text-fg-body"} ${className}`}>
      <span aria-hidden="true" className={`mt-[0.6em] size-[5px] shrink-0 rounded-[1px] ${MARKER[kind]}`} />
      <span className="min-w-0 text-pretty">
        {SPOKEN[kind] && <span className="sr-only">{SPOKEN[kind]}</span>}
        {children}
      </span>
    </li>
  );
}
