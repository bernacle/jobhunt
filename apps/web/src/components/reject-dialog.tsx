"use client";

import { useEffect, useId, useRef, useState } from "react";

import { Button } from "./ui";

/** Starting points, not categories: the text stays the person's own. */
export const REASON_SUGGESTIONS = [
  "Too corporate",
  "Too frontend-heavy",
  "Compensation",
  "Wrong domain",
  "Too much SRE",
  "Company too large",
];

/**
 * "Not for me": asks why, optionally, in the person's own words. Quick
 * suggestions only add words to the text box; nothing restricts what can
 * be said, and nothing is sent until they confirm.
 */
export function RejectDialog({
  open,
  title,
  company,
  onCancel,
  onConfirm,
}: {
  open: boolean;
  title: string;
  company: string;
  onCancel: () => void;
  onConfirm: (reason: string) => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [reason, setReason] = useState("");
  const headingId = useId();
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
      onClose={onCancel}
      onCancel={(e) => {
        e.preventDefault();
        onCancel();
      }}
      className="m-auto w-[min(34rem,calc(100vw-2rem))] rounded-xl border border-line bg-surface p-0 text-ink shadow-xl"
    >
      <form
        method="dialog"
        className="p-5 sm:p-6"
        onSubmit={(e) => {
          e.preventDefault();
          onConfirm(reason);
        }}
      >
        <h2 id={headingId} className="font-serif text-xl">
          Why isn&apos;t this for you?
        </h2>
        <p className="mt-1 text-sm text-muted">
          {title} · {company}
        </p>
        <label htmlFor={fieldId} className="mt-4 block text-sm font-medium">
          Your reason <span className="font-normal text-muted">(optional)</span>
        </label>
        <textarea
          id={fieldId}
          aria-describedby={helpId}
          value={reason}
          onChange={(e) => setReason(e.target.value)}
          rows={3}
          maxLength={500}
          className="mt-1.5 w-full rounded-md border border-line-strong bg-canvas px-3 py-2 text-[0.95rem]"
          placeholder="e.g. too much on-call, and the company is too large"
        />
        <p id={helpId} className="mt-1.5 text-xs text-muted">
          Kept word for word. A reason teaches JobHunt what to avoid; without one, it learns only a little.
        </p>
        <div className="mt-3 flex flex-wrap gap-2" role="group" aria-label="Quick suggestions">
          {REASON_SUGGESTIONS.map((s) => (
            <button
              key={s}
              type="button"
              onClick={() => add(s)}
              className="rounded-full border border-line-strong px-3 py-1 text-sm text-muted hover:bg-sunken hover:text-ink"
            >
              {s}
            </button>
          ))}
        </div>
        <div className="mt-6 flex flex-wrap justify-end gap-2">
          <Button variant="quiet" onClick={onCancel}>
            Cancel
          </Button>
          <Button type="submit" variant="danger">
            Not for me
          </Button>
        </div>
      </form>
    </dialog>
  );
}
