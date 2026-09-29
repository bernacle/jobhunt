"use client";

import Link from "next/link";
import { useActionState, useId, useState, useTransition } from "react";

import type { ActionResult, SourceState } from "@/app/actions";
import type { SourceImportResult, SourceRemovalResult, TallyView } from "@/lib/api-types";

import { Button, Notice, helpClass, inputClass, labelClass, textLinkClass } from "./ui";

type Action = (state: SourceState, form: FormData) => Promise<SourceState>;

function tally(label: string, t: TallyView): string | null {
  const parts = [
    t.added && `${t.added} added`,
    t.corroborated && `${t.corroborated} already in your profile, now also backed by this source`,
    t.updated && `${t.updated} updated`,
    t.restored && `${t.restored} back`,
    t.stale && `${t.stale} no longer in this source`,
  ].filter(Boolean);
  return parts.length ? `${label}: ${parts.join(", ")}` : null;
}

/**
 * What an import changed: what Narrow read, what it left out, and what
 * waits for review. The claims themselves are on the review page.
 */
export function SourceSummary({ result }: { result: SourceImportResult }) {
  const name = result.source === "github" ? "GitHub" : "LinkedIn export";
  const review =
    result.needs_review > 0 ? (
      <p className="mt-2">
        <Link href="/profile/review" className={textLinkClass}>
          Review {result.needs_review} {result.needs_review === 1 ? "claim" : "claims"}
        </Link>{" "}
        — not used until you confirm.
      </p>
    ) : null;
  if (result.unchanged) {
    return (
      <Notice role="status" title={`Same ${name} as before`}>
        Nothing new to read. Your profile is unchanged.
        {review}
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
    <div className="space-y-3">
      <Notice tone="success" role="status" title={result.first_import ? `${name} imported` : `${name} re-imported`}>
        {result.read.length > 0 && <p>Read: {result.read.join(", ")}.</p>}
        {result.skipped.length > 0 && <p>Not used: {result.skipped.join(", ")}.</p>}
        <ul className="mt-1 space-y-0.5">
          {lines.map((l) => (
            <li key={l}>{l}</li>
          ))}
          {kept.length > 0 && <li>{kept.join("; ")}</li>}
          {result.conflicts > 0 && (
            <li>
              {result.conflicts} {result.conflicts === 1 ? "position is" : "positions are"} dated differently than in another source:
              kept apart for you to settle
            </li>
          )}
        </ul>
        {review}
      </Notice>
      {result.problems.length > 0 && (
        <Notice tone="caution" title="Some of it couldn't be read">
          <ul className="space-y-0.5">
            {result.problems.map((p) => (
              <li key={p}>{p}</li>
            ))}
          </ul>
        </Notice>
      )}
    </div>
  );
}

function Outcome({ state }: { state: SourceState }) {
  return (
    <div aria-live="polite">
      {state.error && (
        <Notice tone="error" role="alert" title={state.error.title}>
          {state.error.message}
        </Notice>
      )}
      {state.result && <SourceSummary result={state.result} />}
    </div>
  );
}

const fileInputClass =
  "block w-full min-w-0 cursor-pointer text-[13px] text-fg-secondary file:mr-3 file:h-8 file:cursor-pointer file:rounded-md " +
  "file:border file:border-line file:bg-transparent file:px-3 file:text-ui-m file:text-fg-body hover:file:border-line-strong max-sm:file:h-11";

/** The LinkedIn data export the person downloaded: what Narrow reads, and never. */
export function LinkedinUpload({ action, imported }: { action: Action; imported: boolean }) {
  const [state, formAction, pending] = useActionState(action, {});
  const id = useId();
  const help = useId();
  return (
    <div className="space-y-3">
      <form action={formAction} className="flex flex-col gap-3 sm:flex-row sm:items-end">
        <div className="min-w-0 flex-1">
          <label htmlFor={id} className={labelClass}>
            {imported ? "A newer LinkedIn export" : "Your LinkedIn data export"}
          </label>
          <input id={id} name="export" type="file" required accept=".zip,.csv,application/zip,text/csv" aria-describedby={help} className={fileInputClass} />
        </div>
        <Button type="submit" variant="secondary" disabled={pending} loading={pending} className="max-sm:h-11">
          {imported ? "Re-import" : "Import"}
        </Button>
      </form>
      <p id={help} className={helpClass}>
        Download it from LinkedIn (Settings → Data privacy → Get a copy of your data), then choose the .zip, or one of its CSV files; up to 16 MB.
        Narrow reads only your profile, positions, education, skills, certifications, projects and languages. Messages, connections and
        contacts are never opened. No LinkedIn password or account access.
      </p>
      <Outcome state={state} />
    </div>
  );
}

/** A public GitHub account, through GitHub's API. */
export function GithubImport({ action, login }: { action: Action; login?: string }) {
  const [state, formAction, pending] = useActionState(action, {});
  const id = useId();
  const help = useId();
  return (
    <div className="space-y-3">
      <form action={formAction} className="flex flex-col gap-3 sm:flex-row sm:items-end">
        <div className="min-w-0 flex-1">
          <label htmlFor={id} className={labelClass}>
            GitHub username or profile URL
          </label>
          <input
            id={id}
            name="username"
            type="text"
            required={!login}
            defaultValue={login}
            placeholder="octocat"
            autoComplete="off"
            spellCheck={false}
            aria-describedby={help}
            className={inputClass}
          />
        </div>
        <Button type="submit" variant="secondary" disabled={pending} loading={pending} className="max-sm:h-11">
          {login ? "Re-import" : "Import"}
        </Button>
      </form>
      <p id={help} className={helpClass}>
        Public data only, through GitHub&apos;s API: the repositories you own (not forks), their languages and topics. Stars aren&apos;t read
        as quality and organizations aren&apos;t read as employers. Anything Narrow concludes waits for your review.
      </p>
      <Outcome state={state} />
    </div>
  );
}

/** Takes a LinkedIn export or GitHub account out, after one confirmation. */
export function RemoveSource({
  source,
  label,
  remove,
}: {
  source: "linkedin" | "github";
  label: string;
  remove: (source: "linkedin" | "github") => Promise<ActionResult<SourceRemovalResult>>;
}) {
  const [asking, setAsking] = useState(false);
  const [pending, startTransition] = useTransition();
  const [error, setError] = useState<string | null>(null);
  if (!asking) {
    return (
      <Button size="sm" variant="ghost" className="max-sm:h-11" aria-label={`Remove ${label}`} onClick={() => setAsking(true)}>
        Remove
      </Button>
    );
  }
  return (
    <div className="mt-2 space-y-2" role="group" aria-label={`Remove ${label}`}>
      <p className="text-[13px] text-fg-secondary">
        Remove {label}? What only it supported is deleted; what other sources also say stays, and so do your confirmations and rejections.
      </p>
      <div className="flex gap-1.5">
        <Button
          size="sm"
          variant="destructive"
          className="max-sm:h-11"
          disabled={pending}
          loading={pending}
          onClick={() =>
            startTransition(async () => {
              setError(null);
              const r = await remove(source);
              if (!r.ok) setError(`${r.title}. ${r.message}`);
            })
          }
        >
          Remove {label}
        </Button>
        <Button size="sm" variant="ghost" className="max-sm:h-11" disabled={pending} onClick={() => setAsking(false)}>
          Cancel
        </Button>
      </div>
      {error && (
        <p role="alert" className="text-[13px] text-danger">
          {error}
        </p>
      )}
    </div>
  );
}
