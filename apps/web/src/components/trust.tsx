import type { CompensationView, EligibilityBrief, VerificationBrief } from "@/lib/api-types";
import { compensationLine, eligibilityLine, verificationLine } from "@/lib/format";

/**
 * Pay, eligibility and verification, in plain sentences. Visible but
 * quiet: conditions and unknowns are marked, everything else is text.
 */
export function TrustLines({
  compensation,
  eligibility,
  verification,
  now,
}: {
  compensation: CompensationView;
  eligibility: EligibilityBrief;
  verification: VerificationBrief;
  now?: Date;
}) {
  const pay = compensationLine(compensation);
  const elig = eligibilityLine(eligibility);
  const verified = verificationLine(verification, now);
  return (
    <dl className="grid gap-x-6 gap-y-1 text-sm sm:grid-cols-[auto_1fr]">
      <dt className="text-muted">Pay</dt>
      <dd className={pay.known ? "" : "text-muted"}>
        {pay.text}
        {pay.known && compensation.verified && <span className="text-muted"> · verified</span>}
      </dd>
      <dt className="text-muted">Eligibility</dt>
      <dd className={elig.attention ? "text-caution" : ""}>{elig.text}</dd>
      <dt className="text-muted">Listing</dt>
      <dd className={verification.trusted ? "" : "text-caution"}>{verified}</dd>
    </dl>
  );
}
