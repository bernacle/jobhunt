import type { EligibilityDetail as Detail } from "@/lib/api-types";
import { eligibilityLine } from "@/lib/format";

const VERDICT: Record<string, string> = {
  pass: "Fine",
  conditional: "On a condition",
  unknown: "Unclear",
  fail: "Rules you out",
  not_applicable: "Not relevant",
};

/** The eligibility decision with every reason and the posting's words. */
export function EligibilityDetail({ detail }: { detail: Detail }) {
  const line = eligibilityLine({ status: detail.status, headline: detail.headline });
  const reasons = detail.reasons.filter((r) => r.verdict !== "not_applicable");
  return (
    <div className="space-y-4">
      <p className={line.attention ? "text-caution" : ""}>
        {line.text}
        {detail.option && <span className="text-muted"> ({detail.option})</span>}
      </p>
      {reasons.length > 0 && (
        <ul className="space-y-3">
          {reasons.map((r) => (
            <li key={`${r.rule}-${r.conclusion}`} className="text-sm">
              <p>
                <span className="font-medium">{VERDICT[r.verdict] ?? r.verdict}:</span> {r.conclusion}
              </p>
              {(r.evidence ?? []).length > 0 && (
                <ul className="mt-1 space-y-1 border-l border-line pl-3 text-muted">
                  {(r.evidence ?? []).map((e) => (
                    <li key={e}>
                      <q>{e}</q>
                    </li>
                  ))}
                </ul>
              )}
            </li>
          ))}
        </ul>
      )}
      {(detail.conflicts ?? []).length > 0 && (
        <div className="rounded-md bg-caution-soft px-3 py-2 text-sm">
          <p className="font-medium">The posting contradicts itself</p>
          <ul className="mt-1 list-disc pl-5">
            {(detail.conflicts ?? []).map((c) => (
              <li key={c}>{c}</li>
            ))}
          </ul>
        </div>
      )}
      <p className="text-xs text-muted">{detail.disclaimer}</p>
    </div>
  );
}
