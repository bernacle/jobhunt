import type { ReactNode } from "react";

/*
 * The few ways a screen holds back detail: a row that says a setting's
 * value and one action; a section that starts with its conclusion and
 * opens the rest in place; a state that says one thing and offers one
 * next step. Repeated rows share tracks (globals.css): one label column,
 * one value start, the action on the last track.
 */

/** "Eligibility checks", "Full description": a labelled disclosure that opens in place. */
export function Disclosure({ label, children, className = "" }: { label: ReactNode; children: ReactNode; className?: string }) {
  return (
    <details className={`group ${className}`}>
      <summary className="inline-flex cursor-pointer list-none items-center gap-1.5 text-[13px] font-medium text-fg-secondary transition-colors duration-[120ms] hover:text-fg max-sm:min-h-11 [&::-webkit-details-marker]:hidden">
        <span aria-hidden="true" className="font-mono text-mono-s text-fg-muted transition-transform duration-[120ms] group-open:rotate-90">
          ›
        </span>
        {label}
      </summary>
      <div className="mt-3">{children}</div>
    </details>
  );
}

/**
 * A section of a brief: its label on the shared label track, then the
 * conclusion; the evidence behind it opens in place. Stacked on phones.
 */
export function SummarySection({
  id,
  label,
  conclusion,
  more,
  aside,
  replaces = false,
  children,
}: {
  id: string;
  label: string;
  conclusion: ReactNode;
  /** What the disclosure opens ("Eligibility checks"). */
  more?: string;
  /** A quiet action beside the disclosure ("Open original posting"). */
  aside?: ReactNode;
  /** The opened detail replaces the conclusion (a preview of the same text). */
  replaces?: boolean;
  children?: ReactNode;
}) {
  return (
    <section
      id={id}
      aria-labelledby={`${id}-heading`}
      className={`grid scroll-mt-20 gap-x-4 gap-y-1 border-t border-line-subtle py-5 sm:grid-cols-[var(--nr-track-label)_minmax(0,1fr)] max-sm:py-4 ${
        replaces ? "[&:has(details[open])_[data-conclusion]]:hidden" : ""
      }`}
    >
      <h2 id={`${id}-heading`} className="text-[13px] leading-[1.6] font-medium text-fg-muted">
        {label}
      </h2>
      <div className="min-w-0">
        <div data-conclusion className="max-w-[68ch] text-[15px] leading-[1.55] text-pretty text-fg-body">
          {conclusion}
        </div>
        {((children && more) || aside) && (
          <div className="mt-2 flex flex-wrap items-baseline gap-x-5">
            {children && more && (
              // Open, it takes the whole row (its content lays out by the width it really has).
              <Disclosure label={more} className="min-w-0 open:basis-full">
                {children}
              </Disclosure>
            )}
            {aside}
          </div>
        )}
        {children && !more && <div className="mt-3">{children}</div>}
      </div>
    </section>
  );
}

/**
 * One setting: its name, its current value (with how much it matters only
 * when the value doesn't already say), and one action. An editor, when
 * open, takes the value's place under the same name. On phones the name
 * sits above the value and the action stays within reach on the right.
 */
export function SummaryRow({
  id,
  label,
  value,
  unset = false,
  importance,
  action,
  editor,
  control,
  status,
}: {
  id: string;
  label: string;
  value?: ReactNode;
  unset?: boolean;
  importance?: ReactNode;
  action?: ReactNode;
  /** The open editor, if this row is being edited. */
  editor?: ReactNode;
  /** A control that is its own value (the theme's segmented choice). */
  control?: ReactNode;
  /** The outcome of the last change ("Saved."), next to the value. */
  status?: ReactNode;
}) {
  return (
    <div
      role="group"
      aria-labelledby={id}
      className={
        "grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-4 border-t border-line-subtle py-2.5 max-sm:min-h-[var(--nr-row-min-touch)] " +
        "sm:min-h-[var(--nr-row-min)] sm:grid-cols-[var(--nr-track-label)_minmax(0,1fr)_auto] sm:items-baseline sm:py-3"
      }
    >
      <p id={id} className="text-[12.5px] leading-[1.45] text-fg-muted sm:text-[13.5px] sm:text-fg-secondary">
        {label}
      </p>
      <div className={`min-w-0 max-sm:col-start-1 max-sm:row-start-2 ${editor ? "max-sm:col-span-2" : ""}`}>
        {editor ?? control ?? (
          <p className="flex flex-wrap items-baseline gap-x-2.5 gap-y-0.5">
            <span className={`text-[14px] leading-[1.45] nr-tnum ${unset ? "text-fg-muted" : "font-medium text-fg"}`}>{value}</span>
            {importance && <span className="text-[12.5px] text-fg-muted">{importance}</span>}
            {status}
          </p>
        )}
      </div>
      {!editor && action && <div className="justify-self-end max-sm:col-start-2 max-sm:row-span-2 max-sm:row-start-1">{action}</div>}
    </div>
  );
}

/** A group of rows under a small title ("Work", "Pay"). */
export function RowGroup({ id, title, aside, children }: { id: string; title: string; aside?: ReactNode; children: ReactNode }) {
  return (
    <section id={id} aria-labelledby={`${id}-heading`} className="scroll-mt-20">
      <div className="mb-1.5 flex items-baseline justify-between gap-4">
        <h2 id={`${id}-heading`} className="text-[15px] leading-[1.4] font-semibold tracking-[-0.005em]">
          {title}
        </h2>
        {aside}
      </div>
      <div className="border-b border-line-subtle">{children}</div>
    </section>
  );
}

type StateKind = "gathering" | "empty" | "done" | "error";

const STATE_MARKER: Partial<Record<StateKind, string>> = {
  gathering: "size-[7px] rounded-full bg-accent",
  error: "size-1.5 rounded-[1px] bg-info",
};

/**
 * Loading, empty, caught up or unavailable: one accurate sentence, one
 * useful next step, and technical detail only behind "Details".
 */
export function StateMessage({
  kind,
  title,
  children,
  action,
  detail,
  headingLevel = 2,
  id,
}: {
  kind: StateKind;
  title: string;
  children?: ReactNode;
  action?: ReactNode;
  detail?: ReactNode;
  headingLevel?: 1 | 2;
  id?: string;
}) {
  const Heading = headingLevel === 1 ? "h1" : "h2";
  const marker = STATE_MARKER[kind];
  return (
    <section aria-labelledby={id} role={kind === "error" ? "alert" : undefined} className="max-w-[560px]">
      <Heading id={id} className="flex items-center gap-2.5 text-heading-m max-sm:text-[18px]">
        {marker && <span aria-hidden="true" className={`shrink-0 ${marker}`} />}
        {title}
      </Heading>
      {children && <div className="mt-2 text-body-s text-pretty text-fg-secondary">{children}</div>}
      {(action || detail) && (
        <div className="mt-4 flex flex-wrap items-center gap-x-4 gap-y-2">
          {action}
          {detail && <Disclosure label="Details">{detail}</Disclosure>}
        </div>
      )}
    </section>
  );
}
