import type { FitTier } from "@/lib/api-types";
import { TIER_LABEL } from "@/lib/format";

/**
 * The coarse fit tier JobHunt's ranking returned (never a percentage):
 * words first, a small mark second.
 */
export function TierMark({ tier }: { tier: FitTier }) {
  const strong = tier === "strong_fit";
  return (
    <span className={`inline-flex items-center gap-1.5 text-sm ${strong ? "text-accent font-medium" : "text-muted"}`}>
      <span
        aria-hidden="true"
        className={`inline-block size-2 rounded-full ${strong ? "bg-accent" : "border border-line-strong"}`}
      />
      {TIER_LABEL[tier]}
    </span>
  );
}
