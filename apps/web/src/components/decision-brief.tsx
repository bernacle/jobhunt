import { type Concern, type DecisionInput, type DecisionVariant, concernsOf, selectDecision } from "@/lib/decision";
import { sentence } from "@/lib/format";

import { Consideration } from "./trust";

/** One concern line: a check-first note says so before its words. */
export function ConcernLine({ concern, size = "md", className }: { concern: Concern; size?: "md" | "sm" | "lg"; className?: string }) {
  return (
    <Consideration kind={concern.kind} size={size} className={className}>
      {concern.checkFirst && <span className="font-medium text-fg">Check first: </span>}
      {sentence(concern.text)}
    </Consideration>
  );
}

/**
 * The default view of why an opportunity is here: the strongest distinct
 * reasons and the most material concern, chosen from the API's ordered
 * lists (never all of them). A distinct unresolved stated requirement also
 * appears beside a check-first note. The rest is in the evidence.
 */
export function DecisionSummary({
  input,
  variant = "lead",
  size = "md",
  className = "",
}: {
  input: DecisionInput;
  variant?: DecisionVariant;
  size?: "md" | "sm" | "lg";
  className?: string;
}) {
  const { reasons, concerns, checks } = selectDecision(input, variant);
  return (
    <ul role="list" className={`flex flex-col gap-1.5 ${className}`}>
      {reasons.length > 0 ? (
        reasons.map((line) => (
          <Consideration key={line} kind="reason" size={size}>
            {sentence(line)}
          </Consideration>
        ))
      ) : (
        <li className="text-[14px] text-fg-muted">Nothing specific to you yet.</li>
      )}
      {concerns.map((c) => (
        <ConcernLine key={c.text} concern={c} size={size} />
      ))}
      {checks.length > 0 && (
        <li className="pt-0.5 text-[13px] leading-normal text-fg-muted">
          <span className="font-medium text-fg-secondary">To check:</span> {checks.map((c) => sentence(c)).join(" · ")}
        </li>
      )}
    </ul>
  );
}

/**
 * Everything the brief says, for the evidence: every reason, then every
 * concern in the order the summary picks from. Uncertainty is never folded
 * into the positive side.
 */
export function DecisionBrief({
  why,
  caveats,
  unknowns = [],
  checkFirst,
  eligibilityHeadline,
  headingLevel = 3,
  className = "",
}: {
  why: string[];
  caveats: string[];
  unknowns?: string[];
  /** The ranking's "check this first" note, a caution before the rest. */
  checkFirst?: string | null;
  eligibilityHeadline?: string | null;
  headingLevel?: 2 | 3;
  className?: string;
}) {
  const Heading = headingLevel === 2 ? "h2" : "h3";
  const all = concernsOf({ why, caveats, unknowns, checkFirst, eligibilityHeadline });
  const concerns = all.filter((c) => c.kind === "caution");
  const checks = all.filter((c) => c.kind !== "caution");
  return (
    <div className={`grid gap-x-9 gap-y-6 ${className}`}>
      <div>
        <Heading className="text-label text-fg-muted">Why it may be worth your time</Heading>
        {why.length > 0 ? (
          <ul role="list" className="mt-2.5 flex flex-col gap-1.5">
            {why.map((line) => (
              <Consideration key={line} kind="reason">
                {sentence(line)}
              </Consideration>
            ))}
          </ul>
        ) : (
          <p className="mt-2.5 text-[14px] text-fg-muted">Nothing specific to you yet.</p>
        )}
      </div>
      <div>
        <Heading className="text-label text-fg-muted">Things to consider</Heading>
        {concerns.length === 0 ? (
          <p className="mt-2.5 text-[14px] text-fg-muted">Nothing flagged.</p>
        ) : (
          <ul role="list" className="mt-2.5 flex flex-col gap-1.5">
            {concerns.map((c) => (
              <ConcernLine key={c.text} concern={c} />
            ))}
          </ul>
        )}
      </div>
      {checks.length > 0 && (
        <div>
          <Heading className="text-label text-fg-muted">Things to check</Heading>
          <ul role="list" className="mt-2.5 flex flex-col gap-1.5">
            {checks.map((c) => (
              <ConcernLine key={c.text} concern={c} />
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
