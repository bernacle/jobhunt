"use client";

import { useActionState, useId } from "react";

import type { ResumeState } from "@/app/actions";
import type { ResumeImportResult, TallyView } from "@/lib/api-types";

import { Button, Notice, helpClass, labelClass } from "./ui";

function tally(label: string, t: TallyView): string | null {
  const parts = [
    t.added && `${t.added} added`,
    t.updated && `${t.updated} updated`,
    t.stale && `${t.stale} no longer in the resume`,
    t.restored && `${t.restored} back`,
  ].filter(Boolean);
  return parts.length ? `${label}: ${parts.join(", ")}` : null;
}

/** What an import changed, in a few lines. */
export function ImportSummary({ result }: { result: ResumeImportResult }) {
  if (result.same_file) {
    return (
      <Notice role="status" title="Same resume as before">
        Nothing new to read. Your profile is unchanged.
      </Notice>
    );
  }
  const lines = [
    tally("Experiences", result.experiences),
    tally("Projects", result.projects),
    tally("Education", result.education),
    tally("Skills", result.skills),
    tally("Claims", result.claims),
  ].filter(Boolean) as string[];
  const kept = [
    result.kept_confirmed && `${result.kept_confirmed} confirmed claims kept`,
    result.kept_rejected && `${result.kept_rejected} rejected claims stay rejected`,
    result.preserved_edits && `${result.preserved_edits} of your edits kept`,
  ].filter(Boolean);
  return (
    <Notice tone="success" role="status" title={result.first_import ? "Resume imported" : "Resume re-imported"}>
      <ul className="space-y-0.5">
        {lines.map((l) => (
          <li key={l}>{l}</li>
        ))}
        {kept.length > 0 && <li>{kept.join("; ")}</li>}
        {result.reconfirm > 0 && <li>{result.reconfirm} confirmed claims changed wording: confirm them again</li>}
        {result.needs_review > 0 && <li>{result.needs_review} claims need your review</li>}
      </ul>
      {result.notes.length > 0 && <p className="mt-2">Doubts: {result.notes.join("; ")}</p>}
    </Notice>
  );
}

export function ResumeUpload({
  action,
  hasResume,
}: {
  action: (state: ResumeState, form: FormData) => Promise<ResumeState>;
  hasResume: boolean;
}) {
  const [state, formAction, pending] = useActionState(action, {});
  const id = useId();
  const help = useId();
  return (
    <div className="space-y-3">
      <form action={formAction} className="flex flex-col gap-3 sm:flex-row sm:items-end">
        <div className="min-w-0 flex-1">
          <label htmlFor={id} className={labelClass}>
            {hasResume ? "Replace with an updated resume" : "Your resume"}
          </label>
          <input
            id={id}
            name="resume"
            type="file"
            required
            accept=".pdf,.txt,.md,application/pdf,text/plain,text/markdown"
            aria-describedby={help}
            className={
              "block w-full min-w-0 cursor-pointer text-[13px] text-fg-secondary file:mr-3 file:h-8 file:cursor-pointer file:rounded-md " +
              "file:border file:border-line file:bg-transparent file:px-3 file:text-ui-m file:text-fg-body hover:file:border-line-strong max-sm:file:h-11"
            }
          />
        </div>
        <Button type="submit" variant={hasResume ? "secondary" : "primary"} disabled={pending} loading={pending} className="max-sm:h-11">
          {hasResume ? "Re-import" : "Import"}
        </Button>
      </form>
      <p id={help} className={helpClass}>
        PDF, .txt or .md, up to 10 MB. Read on Narrow&apos;s server, never by an AI service.
        {hasResume && " Re-importing keeps your edits, confirmations and rejections."}
      </p>
      <div aria-live="polite">
        {state.error && (
          <Notice tone="error" role="alert" title={state.error.title}>
            {state.error.message}
          </Notice>
        )}
        {state.result && <ImportSummary result={state.result} />}
      </div>
    </div>
  );
}
