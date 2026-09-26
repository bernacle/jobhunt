import type { ReactNode } from "react";

import type { SourceRecordView, VerificationBrief } from "@/lib/api-types";
import { ago, authorityLabel, sourceLabel, sourceMark } from "@/lib/format";

import { Consideration, VerificationGlyph } from "./trust";
import { textLinkClass } from "./ui";

/** One line of the opportunity page's aside: a glyph, a fact, and when (in mono). */
export function CheckedLine({ glyph, when, children }: { glyph: ReactNode; when?: ReactNode; children: ReactNode }) {
  return (
    <li className="grid grid-cols-[14px_minmax(0,1fr)] gap-1.5 text-[13px] leading-[1.45] text-fg-body">
      <span className="text-[12px]">{glyph}</span>
      <span className="min-w-0">
        {children}
        {when && <span className="mt-0.5 block font-mono text-mono-s text-fg-muted">{when}</span>}
      </span>
    </li>
  );
}

/**
 * Where the listing comes from, record by record. The check follows the
 * listing's verification (trust and freshness, as the API judged them) and
 * only on the record it rests on: a stale or secondary record never looks
 * as current as an authoritative one.
 */
export function Provenance({ sources, verification, now }: { sources: SourceRecordView[]; verification: VerificationBrief; now?: Date }) {
  return (
    <ul role="list" className="flex flex-col gap-3">
      {sources.map((s) => (
        <CheckedLine
          key={s.job_id}
          glyph={<VerificationGlyph mark={sourceMark(s, verification)} />}
          when={
            <>
              {s.status === "open" ? "listed" : "closed"} · first seen {ago(s.first_seen_at, now)}
              {s.last_success_at && <> · last verified {ago(s.last_success_at, now)}</>}
            </>
          }
        >
          <a href={s.url} target="_blank" rel="noopener noreferrer" className={textLinkClass}>
            {sourceLabel(s.source)}
            <span className="sr-only"> (opens a new tab)</span>
          </a>
          <span className="block text-fg-secondary">Published by {authorityLabel(s.authority)}</span>
          {s.last_attempt?.failure && (
            <ul role="list" className="mt-1">
              <Consideration kind="caution" size="sm">
                Last check failed: {s.last_attempt.failure}
              </Consideration>
            </ul>
          )}
        </CheckedLine>
      ))}
    </ul>
  );
}
