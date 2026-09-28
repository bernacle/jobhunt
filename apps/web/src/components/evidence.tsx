import type { ReactNode } from "react";

/*
 * The parts of an evidence panel: labelled sections of quotes, checks and
 * facts. Checks and facts share one value track, so values start at one
 * edge however long their labels are.
 */

export function EvidenceSection({ label, children }: { label: string; children: ReactNode }) {
  return (
    <section className="min-w-0">
      <h3 className="mb-2.5 text-label text-fg-muted">{label}</h3>
      <div className="flex flex-col gap-2.5">{children}</div>
    </section>
  );
}

/** Someone's own words, verbatim, with where they are from. */
export function EvidenceQuote({ children, source }: { children: ReactNode; source?: ReactNode }) {
  return (
    <figure className="m-0 rounded-md border border-line-subtle bg-inset px-3.5 py-3">
      <blockquote className="text-[14px] leading-[1.55] text-pretty text-fg-body">
        <q>{children}</q>
      </blockquote>
      {source && <figcaption className="mt-2 font-mono text-mono-xs text-fg-muted">{source}</figcaption>}
    </figure>
  );
}

const CHECK_MARKER = {
  pass: "bg-fg-secondary",
  condition: "bg-warning",
  fail: "bg-danger",
  unknown: "border border-fg-muted",
};

const CHECK_SPOKEN = { pass: "Fine: ", condition: "On a condition: ", fail: "Rules you out: ", unknown: "Unclear: " };

/** One check: a marker, what was checked, and what it found. */
export function EvidenceCheck({ label, children, state = "pass" }: { label: ReactNode; children: ReactNode; state?: keyof typeof CHECK_MARKER }) {
  return (
    <div className="grid grid-cols-[14px_minmax(0,1fr)] gap-x-1.5 gap-y-0.5 text-[13.5px] leading-normal sm:grid-cols-[14px_calc(var(--nr-track-label-evidence)-20px)_minmax(0,1fr)]">
      <span aria-hidden="true" className={`mt-[0.55em] size-1.5 rounded-[1px] ${CHECK_MARKER[state]}`} />
      <span className="text-fg-secondary">
        <span className="sr-only">{CHECK_SPOKEN[state]}</span>
        {label}
      </span>
      <span className={`min-w-0 max-sm:col-start-2 ${state === "unknown" ? "text-fg-secondary" : "text-fg-body"}`}>{children}</span>
    </div>
  );
}

/** A labelled fact inside evidence. */
export function EvidenceFact({ label, children }: { label: ReactNode; children: ReactNode }) {
  return (
    <div className="grid gap-x-1.5 gap-y-0.5 text-[13.5px] leading-normal sm:grid-cols-[var(--nr-track-label-evidence)_minmax(0,1fr)]">
      <span className="text-fg-muted">{label}</span>
      <span className="min-w-0 text-fg-body">{children}</span>
    </div>
  );
}
