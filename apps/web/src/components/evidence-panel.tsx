"use client";

import { type ReactNode, useRef, useState } from "react";

import { Sheet, SheetBody } from "./sheet";
import { inlineActionClass } from "./ui";

/**
 * Evidence on demand: a 440px panel at the side on wide screens, a
 * full-height sheet on phones (the shared `Sheet`). Its conclusion comes
 * first, then the detail behind it.
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
  return (
    <Sheet open={open} onClose={onClose} title={title} subtitle={subtitle}>
      <SheetBody className="gap-7 pb-10 max-sm:pb-[calc(32px+env(safe-area-inset-bottom))]">
        {conclusion && <div className="text-[15px] leading-[1.55] text-pretty text-fg-body">{conclusion}</div>}
        {children}
      </SheetBody>
    </Sheet>
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
