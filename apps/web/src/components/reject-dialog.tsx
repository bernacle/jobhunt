"use client";

import { type ReactNode, useEffect, useId, useRef, useState } from "react";

import { Button, helpClass, labelClass, textareaClass } from "./ui";

/**
 * Starting points, not categories: one click adds the words, the text stays
 * the person's own (anything else goes in the box). Each is read into what
 * Narrow learns from this job ("Too specialized" is the job's specialty).
 */
export const REASON_SUGGESTIONS = [
  "Wrong seniority",
  "Too specialized",
  "Wrong kind of work",
  "Company too big",
  "Company stage",
  "Domain",
  "Location",
  "Compensation",
];

/**
 * "Not for me": asks why, optionally, in the person's own words. Quick
 * suggestions only add words to the text box; nothing restricts what can
 * be said, and nothing is sent until they confirm. A native modal dialog
 * (focus moves in, Escape closes); a bottom sheet on phones.
 */
export function RejectDialog({
  open,
  title,
  company,
  onCancel,
  onConfirm,
  heading = "Not for me",
  description,
  confirmLabel = "Mark not for me",
  notNowHint = true,
}: {
  open: boolean;
  title: string;
  company: string;
  onCancel: () => void;
  onConfirm: (reason: string) => void;
  heading?: string;
  description?: ReactNode;
  confirmLabel?: string;
  /** Point to "Not now" for someone who only wants it out of the way. */
  notNowHint?: boolean;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [reason, setReason] = useState("");
  const headingId = useId();
  const descriptionId = useId();
  const fieldId = useId();
  const helpId = useId();

  useEffect(() => {
    const d = dialog.current;
    if (!d) return;
    if (open && !d.open) {
      setReason("");
      d.showModal();
    } else if (!open && d.open) {
      d.close();
    }
  }, [open]);

  const add = (suggestion: string) =>
    setReason((current) => {
      const trimmed = current.trim();
      if (trimmed.toLowerCase().includes(suggestion.toLowerCase())) return current;
      return trimmed ? `${trimmed}; ${suggestion.toLowerCase()}` : suggestion;
    });

  return (
    <dialog
      ref={dialog}
      aria-labelledby={headingId}
      aria-describedby={descriptionId}
      onClose={onCancel}
      onCancel={(e) => {
        e.preventDefault();
        onCancel();
      }}
      className={
        "m-auto w-[min(30rem,calc(100vw-2rem))] rounded-lg bg-overlay p-0 text-fg shadow-overlay open:animate-[nr-rise_240ms_var(--nr-ease-out)] " +
        "max-sm:mb-0 max-sm:w-full max-sm:max-w-full max-sm:rounded-b-none"
      }
    >
      <form
        method="dialog"
        onSubmit={(e) => {
          e.preventDefault();
          onConfirm(reason);
        }}
      >
        <div className="flex items-baseline justify-between gap-4 px-5 pt-5">
          <h2 id={headingId} className="text-[18px] font-semibold tracking-[-0.015em]">
            {heading}
          </h2>
          <kbd aria-hidden="true" className="font-mono text-mono-xs text-fg-muted max-sm:hidden">
            esc
          </kbd>
        </div>
        <p id={descriptionId} className="px-5 pt-2 text-[14px] leading-normal text-pretty text-fg-secondary">
          {description ?? (
            <>
              <span className="text-fg-body">
                {title} · {company}
              </span>{" "}
              won&apos;t be recommended again. What didn&apos;t fit? Narrow weighs it in future rankings.
            </>
          )}
        </p>
        <div className="px-5 pt-4 pb-5">
          <label htmlFor={fieldId} className={labelClass}>
            What didn&apos;t fit? <span className="text-fg-muted">(optional)</span>
          </label>
          <textarea
            id={fieldId}
            aria-describedby={helpId}
            value={reason}
            onChange={(e) => setReason(e.target.value)}
            rows={3}
            maxLength={500}
            className={textareaClass}
            placeholder="e.g. too much on-call, and the company is too large"
          />
          <p id={helpId} className={helpClass}>
            Kept word for word. A reason teaches Narrow what to avoid; without one, it learns only a little.
          </p>
          <div className="mt-3 flex flex-wrap gap-1.5" role="group" aria-label="Quick suggestions">
            {REASON_SUGGESTIONS.map((s) => (
              <Button key={s} size="sm" variant="secondary" className="max-sm:h-9" onClick={() => add(s)}>
                {s}
              </Button>
            ))}
          </div>
          {notNowHint && (
            <p className="mt-4 text-[12.5px] leading-normal text-fg-muted">
              Just want it out of the way today? Use Not now instead. It won&apos;t change what Narrow has learned.
            </p>
          )}
        </div>
        <div className="flex justify-end gap-2 border-t border-line-subtle px-5 py-3.5 max-sm:grid max-sm:grid-cols-[1fr_1.4fr] max-sm:pb-7">
          <Button variant="ghost" className="max-sm:h-11" onClick={onCancel}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" className="max-sm:h-11">
            {confirmLabel}
          </Button>
        </div>
      </form>
    </dialog>
  );
}
