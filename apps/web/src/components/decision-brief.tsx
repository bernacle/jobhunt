import { sentence } from "@/lib/format";

/**
 * The decision brief: why an opportunity may be worth the person's time,
 * then what to consider. Uncertainty is never folded into the positive
 * side: every caveat and unknown the ranking returned is listed.
 */
export function DecisionBrief({
  why,
  consider,
  unknowns = [],
  headingLevel = 3,
}: {
  why: string[];
  consider: string[];
  unknowns?: string[];
  headingLevel?: 2 | 3;
}) {
  const Heading = headingLevel === 2 ? "h2" : "h3";
  const caveats = [...consider, ...unknowns.filter((u) => !consider.includes(u))];
  return (
    <div className="grid gap-5 sm:grid-cols-2 sm:gap-8">
      <div>
        <Heading className="text-xs font-semibold uppercase tracking-wider text-muted">
          Why this may be worth your time
        </Heading>
        {why.length > 0 ? (
          <ul className="mt-2 space-y-1.5 text-[0.95rem]">
            {why.map((line) => (
              <li key={line} className="flex gap-2">
                <span aria-hidden="true" className="mt-2.5 size-1 shrink-0 rounded-full bg-accent" />
                <span>{sentence(line)}</span>
              </li>
            ))}
          </ul>
        ) : (
          <p className="mt-2 text-sm text-muted">Nothing specific to you yet.</p>
        )}
      </div>
      <div>
        <Heading className="text-xs font-semibold uppercase tracking-wider text-muted">Things to consider</Heading>
        {caveats.length > 0 ? (
          <ul className="mt-2 space-y-1.5 text-[0.95rem]">
            {caveats.map((line) => (
              <li key={line} className="flex gap-2">
                <span aria-hidden="true" className="mt-2 inline-block h-px w-2 shrink-0 bg-caution" />
                <span>{sentence(line)}</span>
              </li>
            ))}
          </ul>
        ) : (
          <p className="mt-2 text-sm text-muted">Nothing flagged.</p>
        )}
      </div>
    </div>
  );
}
