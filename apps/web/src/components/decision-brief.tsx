import { sentence } from "@/lib/format";

import { Consideration } from "./trust";

/**
 * Why an opportunity may be worth the person's time, then what to
 * consider. Uncertainty is never folded into the positive side: every
 * caveat (a caution, sand) and unknown (not stated, hollow) the ranking
 * returned is listed. `compact` is the peer treatment: denser, with the
 * labels kept for screen readers only.
 */
export function DecisionBrief({
  why,
  caveats,
  unknowns = [],
  checkFirst,
  headingLevel = 3,
  compact = false,
  className = "",
}: {
  why: string[];
  caveats: string[];
  unknowns?: string[];
  /** The ranking's "check this first" note, a caution before the rest. */
  checkFirst?: string | null;
  headingLevel?: 2 | 3;
  compact?: boolean;
  className?: string;
}) {
  const Heading = headingLevel === 2 ? "h2" : "h3";
  const cautions = caveats.filter((c) => !unknowns.includes(c));
  const missing = unknowns.filter((u, i) => unknowns.indexOf(u) === i);
  const nothing = cautions.length === 0 && missing.length === 0 && !checkFirst;
  const label = compact ? "sr-only" : "text-label text-fg-muted";
  const size = compact ? "sm" : "md";
  return (
    <div
      className={`grid gap-x-9 md:grid-cols-[minmax(0,1.15fr)_minmax(0,1fr)] ${compact ? "gap-y-1.5" : "gap-y-6"} ${className}`}
    >
      <div>
        <Heading className={label}>Why it may be worth your time</Heading>
        {why.length > 0 ? (
          <ul role="list" className={`${compact ? "text-row" : "mt-2.5 text-body-m"} space-y-1 text-pretty text-fg-body`}>
            {why.map((line) => (
              <li key={line}>{sentence(line)}</li>
            ))}
          </ul>
        ) : (
          <p className={`${compact ? "" : "mt-2.5"} text-[14px] text-fg-muted`}>Nothing specific to you yet.</p>
        )}
      </div>
      <div>
        <Heading className={label}>Things to consider</Heading>
        {nothing ? (
          <p className={`${compact ? "" : "mt-2.5"} text-[14px] text-fg-muted`}>Nothing flagged.</p>
        ) : (
          <ul role="list" className={`${compact ? "space-y-1" : "mt-2.5 space-y-2"}`}>
            {checkFirst && (
              <Consideration kind="caution" size={size}>
                <span className="font-medium text-fg">Check first:</span> {sentence(checkFirst)}
              </Consideration>
            )}
            {cautions.map((line) => (
              <Consideration key={line} kind="caution" size={size}>
                {sentence(line)}
              </Consideration>
            ))}
            {missing.map((line) => (
              <Consideration key={line} kind="missing" size={size}>
                {sentence(line)}
              </Consideration>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
