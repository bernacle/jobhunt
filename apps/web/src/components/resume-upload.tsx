"use client";

import { useActionState, useId } from "react";

import type { ResumeState } from "@/app/actions";
import type { ResumeImportResult, TallyView } from "@/lib/api-types";

import { Button, Notice } from "./ui";

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
      <ul className="list-disc space-y-0.5 pl-5">
        {lines.map((l) => (
          <li key={l}>{l}</li>
        ))}
        {kept.length > 0 && <li>{kept.join("; ")}</li>}
        {result.reconfirm > 0 && <li>{result.reconfirm} confirmed claims changed wording: confirm them again</li>}
        {result.needs_review > 0 && <li>{result.needs_review} claims need your review below</li>}
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
        <div className="flex-1">
          <label htmlFor={id} className="block text-sm font-medium">
            {hasResume ? "Replace with an updated resume" : "Your resume"}
          </label>
          <input
            id={id}
            name="resume"
            type="file"
            required
            accept=".pdf,.txt,.md,application/pdf,text/plain,text/markdown"
            aria-describedby={help}
            className="mt-1.5 block w-full text-sm file:mr-3 file:rounded-md file:border file:border-line-strong file:bg-surface file:px-3 file:py-1.5 file:text-sm"
          />
        </div>
        <Button type="submit" variant="primary" disabled={pending}>
          {pending ? "Reading…" : hasResume ? "Re-import" : "Import"}
        </Button>
      </form>
      <p id={help} className="text-xs text-muted">
        PDF, .txt or .md, up to 10 MB. Read on JobHunt&apos;s server, never by an AI service.
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
