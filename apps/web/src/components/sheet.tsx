"use client";

import { type KeyboardEvent, type ReactNode, useEffect, useId, useRef } from "react";

/**
 * One focused surface over the page: a 440px panel at the side on wide
 * screens, a sheet from the bottom on phones. A native modal dialog, so
 * focus moves in, Escape closes, and the page behind is inert; whoever
 * opened it puts focus back. Its content is rendered only while it is
 * open. Evidence (`EvidencePanel`) and one preference being changed (a
 * Preferences row's editor) are the two things that use it.
 *
 * `fit` sizes a phone sheet to its content (an editor with a few choices)
 * instead of the full height (evidence to read).
 */
export function Sheet({
  open,
  onClose,
  title,
  subtitle,
  fit = false,
  closeWord = true,
  initialFocus,
  children,
}: {
  open: boolean;
  onClose: () => void;
  title: string;
  subtitle?: ReactNode;
  fit?: boolean;
  /** On a phone, the close button says "Done" (evidence); an editor has its own Cancel, so it shows ×. */
  closeWord?: boolean;
  /** Where focus goes once it is open (the current choice); else the Close button. */
  initialFocus?: (panel: HTMLElement) => HTMLElement | null | undefined;
  children: ReactNode;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const closeButton = useRef<HTMLButtonElement>(null);
  const titleId = useId();
  // Whether it is meant to be open: a close the browser does on its own
  // (a form's dialog method, the platform) reaches `onClose`; closing it
  // because `open` turned false doesn't call it a second time.
  const wanted = useRef(open);

  useEffect(() => {
    wanted.current = open;
    const d = dialog.current;
    if (!d) return;
    if (open && !d.open) {
      d.showModal();
      // Into the sheet: where the caller says, else its Close button.
      const target = (panel.current && initialFocus?.(panel.current)) || closeButton.current;
      target?.focus();
    } else if (!open && d.open) d.close();
    // Focus is placed once, when it opens.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  // Escape is the same as Cancel. Handled here (and the browser's own
  // close request prevented) so it runs once, in every browser.
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onClose();
    }
  };

  return (
    <dialog
      ref={dialog}
      aria-labelledby={titleId}
      onClose={() => {
        if (wanted.current) onClose();
      }}
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
      onKeyDown={onKeyDown}
      // A click on the backdrop (the dialog element itself, outside the panel) closes it.
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
      className={
        "fixed inset-y-0 right-0 left-auto m-0 h-dvh max-h-none w-[440px] max-w-full bg-transparent p-0 text-fg " +
        `max-sm:inset-x-0 max-sm:top-auto max-sm:bottom-0 max-sm:w-full ${fit ? "max-sm:h-auto max-sm:max-h-[calc(100dvh-3rem)]" : "max-sm:h-[calc(100dvh-3rem)]"}`
      }
    >
      {open && (
        <div
          ref={panel}
          className={`flex h-full flex-col bg-overlay shadow-overlay animate-[nr-rise_240ms_var(--nr-ease-out)] max-sm:rounded-t-lg ${fit ? "max-sm:max-h-[calc(100dvh-3rem)]" : ""}`}
        >
          <div className="flex shrink-0 items-start justify-between gap-4 border-b border-line-subtle py-4 pr-4 pl-6 max-sm:py-2 max-sm:pr-2 max-sm:pl-5">
            <div className="min-w-0 pt-1 max-sm:pt-2.5 max-sm:pb-1.5">
              <h2 id={titleId} className="text-[16px] leading-[1.35] font-semibold tracking-[-0.01em] text-pretty">
                {title}
              </h2>
              {subtitle && <p className="mt-0.5 line-clamp-2 text-[12.5px] leading-[1.45] text-pretty text-fg-muted">{subtitle}</p>}
            </div>
            <button
              ref={closeButton}
              type="button"
              onClick={onClose}
              aria-label="Close"
              className={
                "grid size-7 shrink-0 cursor-pointer place-items-center rounded-md border border-line text-[14px] text-fg-secondary hover:border-line-strong hover:text-fg " +
                (closeWord ? "max-sm:h-11 max-sm:w-auto max-sm:border-0 max-sm:px-3 max-sm:text-[14px] max-sm:font-medium" : "max-sm:size-11 max-sm:border-0 max-sm:text-[18px]")
              }
            >
              <span aria-hidden="true" className={closeWord ? "max-sm:hidden" : ""}>
                ×
              </span>
              {closeWord && (
                <span aria-hidden="true" className="sm:hidden">
                  Done
                </span>
              )}
            </button>
          </div>
          {children}
        </div>
      )}
    </dialog>
  );
}

/** The sheet's content, scrolling on its own under the fixed header. The caller sets its rhythm (gap, bottom room). */
export function SheetBody({ children, className = "" }: { children: ReactNode; className?: string }) {
  return (
    <div className={`flex min-h-0 flex-1 flex-col overflow-y-auto px-6 pt-6 max-sm:px-5 max-sm:pt-5 ${className}`}>
      {children}
    </div>
  );
}

/**
 * What to do with the sheet (Save, Cancel), always in view at its foot,
 * and within thumb reach on a phone.
 */
export function SheetFooter({ children }: { children: ReactNode }) {
  return (
    <div className="flex shrink-0 flex-wrap items-center gap-2 border-t border-line-subtle px-6 py-3.5 max-sm:px-5 max-sm:pt-3 max-sm:pb-[calc(12px+env(safe-area-inset-bottom))]">
      {children}
    </div>
  );
}
