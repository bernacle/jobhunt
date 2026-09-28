"use client";

import { type ReactNode, useEffect, useId, useRef, useState } from "react";

import { inlineActionClass } from "./ui";

/**
 * Evidence on demand: a 440px panel at the side on wide screens, a
 * full-height sheet on phones. A native modal dialog, so focus moves in,
 * Escape closes, and the page behind is inert; focus goes back to what
 * opened it. The content is rendered only while it is open.
 */
export function EvidencePanel({
  open,
  onClose,
  title,
  subtitle,
  conclusion,
  children,
}: {
  open: boolean;
  onClose: () => void;
  title: string;
  subtitle?: ReactNode;
  conclusion?: ReactNode;
  children: ReactNode;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const titleId = useId();

  useEffect(() => {
    const d = dialog.current;
    if (!d) return;
    if (open && !d.open) d.showModal();
    else if (!open && d.open) d.close();
  }, [open]);

  return (
    <dialog
      ref={dialog}
      aria-labelledby={titleId}
      onClose={onClose}
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
      // A click on the backdrop (the dialog element itself, outside the panel) closes it.
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
      className={
        "fixed inset-y-0 right-0 left-auto m-0 h-dvh max-h-none w-[440px] max-w-full bg-transparent p-0 text-fg " +
        "max-sm:inset-x-0 max-sm:top-auto max-sm:bottom-0 max-sm:h-[calc(100dvh-3rem)] max-sm:w-full"
      }
    >
      {open && (
        <div className="flex h-full flex-col bg-overlay shadow-overlay animate-[nr-rise_240ms_var(--nr-ease-out)] max-sm:rounded-t-lg">
          <div className="flex items-start justify-between gap-4 border-b border-line-subtle py-4 pr-4 pl-6 max-sm:py-2 max-sm:pr-2 max-sm:pl-5">
            <div className="min-w-0 pt-1 max-sm:pt-2.5 max-sm:pb-1.5">
              <h2 id={titleId} className="text-[16px] leading-[1.35] font-semibold tracking-[-0.01em]">
                {title}
              </h2>
              {subtitle && <p className="mt-0.5 text-[12.5px] leading-[1.45] text-fg-muted">{subtitle}</p>}
            </div>
            <button
              type="button"
              onClick={onClose}
              aria-label="Close"
              className="grid size-7 shrink-0 cursor-pointer place-items-center rounded-md border border-line text-[14px] text-fg-secondary hover:border-line-strong hover:text-fg max-sm:h-11 max-sm:w-auto max-sm:border-0 max-sm:px-3 max-sm:text-[14px] max-sm:font-medium"
            >
              <span aria-hidden="true" className="max-sm:hidden">
                ×
              </span>
              <span aria-hidden="true" className="sm:hidden">
                Done
              </span>
            </button>
          </div>
          <div className="flex min-h-0 flex-1 flex-col gap-7 overflow-y-auto px-6 pt-6 pb-10 max-sm:px-5 max-sm:pt-5 max-sm:pb-[calc(32px+env(safe-area-inset-bottom))]">
            {conclusion && <div className="text-[15px] leading-[1.55] text-pretty text-fg-body">{conclusion}</div>}
            {children}
          </div>
        </div>
      )}
    </dialog>
  );
}

/**
 * A labelled action that opens an evidence panel ("See evidence",
 * "How preferences work"), and returns focus to itself when it closes.
 */
export function EvidenceTrigger({
  label,
  srLabel,
  className = "",
  ...panel
}: {
  label: ReactNode;
  /** More words for screen readers ("See evidence for …"). */
  srLabel?: string;
  className?: string;
  title: string;
  subtitle?: ReactNode;
  conclusion?: ReactNode;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  return (
    <>
      <button
        ref={trigger}
        type="button"
        aria-haspopup="dialog"
        onClick={() => setOpen(true)}
        className={`${inlineActionClass} text-[13px] max-sm:min-h-11 ${className}`}
      >
        {label}
        {srLabel && (
          <>
            {" "}
            <span className="sr-only">{srLabel}</span>
          </>
        )}
      </button>
      <EvidencePanel
        {...panel}
        open={open}
        onClose={() => {
          setOpen(false);
          trigger.current?.focus();
        }}
      />
    </>
  );
}
