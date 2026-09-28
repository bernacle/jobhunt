"use client";

import { useActionState, useId, useState, useTransition } from "react";

import type { ActionResult, StatementState } from "@/app/actions";
import type { PreferenceInput, PreferenceUpdateResult, PreferenceView, StatementView } from "@/lib/api-types";
import { STANCE_LABEL } from "@/lib/format";

import { Disclosure } from "./summary";
import { Button, Notice, helpClass, inlineActionClass, inputClass, labelClass, selectClass, textareaClass } from "./ui";

function PreferenceLine({ p }: { p: PreferenceView }) {
  return (
    <>
      <span className="font-medium">{STANCE_LABEL[p.stance] ?? p.stance}:</span> {p.value}
    </>
  );
}

type Kind = "role" | "domain" | "work_style" | "company" | "timezone";

// What a preference can be about, beyond the structured settings above it
// (work setup, location, pay, company and team): the same model the API
// accepts.
const KINDS: { kind: Kind; label: string; placeholder: string }[] = [
  { kind: "role", label: "Role", placeholder: "backend, platform, founding engineer" },
  { kind: "domain", label: "Domain or product", placeholder: "developer tools, fintech" },
  { kind: "work_style", label: "Way of working", placeholder: "ownership, greenfield, async-communication" },
  { kind: "company", label: "Kind of company", placeholder: "founder-led, product-company, remote-first" },
  { kind: "timezone", label: "Time zone", placeholder: "UTC-3, US hours" },
];

const STANCES = [
  { value: "require", label: "Must have" },
  { value: "want", label: "Want" },
  { value: "accept", label: "Fine with" },
  { value: "avoid", label: "Avoid" },
];

type Stance = "require" | "want" | "accept" | "avoid";

function toInput(kind: Kind, value: string, stance: Stance): PreferenceInput {
  switch (kind) {
    case "role":
      return { kind, role: value, stance };
    case "domain":
      return { kind, domain: value, stance };
    case "company":
      return { kind, company: value, stance };
    case "work_style":
      return { kind, aspect: value, stance };
    case "timezone":
      return { kind, zone: value, stance };
  }
}

/**
 * A precise preference: what it's about, the rule, the value. The same
 * structured model the API, the CLI and AI assistants use; no free text
 * is parsed here.
 */
export function AddPreference({ set }: { set: (input: PreferenceInput) => Promise<ActionResult<PreferenceUpdateResult>> }) {
  const [kind, setKind] = useState<Kind>("role");
  const [pending, startTransition] = useTransition();
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);
  const ids = { kind: useId(), rule: useId(), value: useId(), help: useId() };
  const current = KINDS.find((k) => k.kind === kind)!;

  return (
    <form
      aria-describedby={ids.help}
      className="grid gap-3 md:grid-cols-[180px_minmax(0,1fr)_auto] md:items-end"
      onSubmit={(e) => {
        e.preventDefault();
        setMessage(null);
        const data = new FormData(e.currentTarget);
        const value = String(data.get("value") ?? "").trim();
        const rule = String(data.get("rule") ?? "want") as Stance;
        if (!value) return;
        startTransition(async () => {
          const r = await set(toInput(kind, value, rule));
          setMessage(r.ok ? { ok: true, text: r.data.unchanged ? "Already in effect." : "Saved." } : { ok: false, text: `${r.title}. ${r.message}` });
        });
      }}
    >
      <div className="min-w-0">
        <label htmlFor={ids.kind} className={labelClass}>
          About
        </label>
        <select id={ids.kind} value={kind} onChange={(e) => setKind(e.target.value as Kind)} className={selectClass}>
          {KINDS.map((k) => (
            <option key={k.kind} value={k.kind}>
              {k.label}
            </option>
          ))}
        </select>
      </div>
      <div className="grid min-w-0 gap-3 sm:grid-cols-[140px_minmax(0,1fr)]">
        <div className="min-w-0">
          <label htmlFor={ids.rule} className={labelClass}>
            Rule
          </label>
          <select id={ids.rule} name="rule" defaultValue="want" className={selectClass}>
            {STANCES.map((s) => (
              <option key={s.value} value={s.value}>
                {s.label}
              </option>
            ))}
          </select>
        </div>
        <div className="min-w-0">
          <label htmlFor={ids.value} className={labelClass}>
            Value
          </label>
          <input id={ids.value} name="value" required placeholder={current.placeholder} className={inputClass} />
        </div>
      </div>
      <Button type="submit" variant="secondary" disabled={pending} loading={pending} className="h-9 max-sm:h-11 max-sm:w-full">
        Add preference
      </Button>
      <p id={ids.help} className={`${helpClass} empty:hidden md:col-span-3 ${message?.ok === false ? "text-danger" : ""}`} aria-live="polite">
        {message?.text}
      </p>
    </form>
  );
}

/** Removes one preference (its id), after the API confirms. */
export function RemovePreference({
  id,
  label,
  remove,
}: {
  id: string;
  label: string;
  remove: (id: string) => Promise<ActionResult<PreferenceUpdateResult>>;
}) {
  const [pending, startTransition] = useTransition();
  const [error, setError] = useState<string | null>(null);
  return (
    <>
      <button
        type="button"
        disabled={pending}
        onClick={() =>
          startTransition(async () => {
            const r = await remove(id);
            if (!r.ok) setError(r.message);
          })
        }
        className={`${inlineActionClass} max-sm:min-h-11`}
      >
        {pending ? "Removing…" : "Remove"}
        <span className="sr-only"> {label}</span>
      </button>
      {error && (
        <span role="alert" className="ml-2 text-[13px] text-danger">
          {error}
        </span>
      )}
    </>
  );
}

/**
 * What the person said, newest first, and a way to say more. The words
 * are kept as written; what Narrow couldn't interpret is one disclosure
 * away. With nothing said yet, the box is simply open.
 */
export function InYourWords({
  statements,
  action,
}: {
  statements: StatementView[];
  action: (state: StatementState, form: FormData) => Promise<StatementState>;
}) {
  const [adding, setAdding] = useState(statements.length === 0);
  const newest = [...statements].reverse();
  const shown = newest.slice(0, 3);
  const earlier = newest.slice(3);
  const line = (s: StatementView) => (
    <li key={s.id} className="border-t border-line-subtle py-3.5">
      <q className="text-[15px] leading-[1.55] text-pretty text-fg-body">{s.text}</q>
      <div className="mt-1 flex flex-wrap items-center gap-x-4">
        <time dateTime={s.at} className="font-mono text-mono-s text-fg-muted">
          {new Date(s.at).toLocaleDateString("en-GB", { day: "numeric", month: "short", timeZone: "UTC" })}
        </time>
        {s.not_understood.length > 0 && (
          <Disclosure label="How Narrow read this">
            <p className="text-[13px] text-fg-secondary">
              Not interpreted, kept as written: {s.not_understood.map((part) => `“${part}”`).join(", ")}
            </p>
          </Disclosure>
        )}
      </div>
    </li>
  );
  return (
    <section id="statements" aria-labelledby="statements-heading" className="scroll-mt-20">
      <div className="mb-1.5 flex items-baseline justify-between gap-4">
        <h2 id="statements-heading" className="text-[15px] leading-[1.4] font-semibold tracking-[-0.005em]">
          In your words
        </h2>
        {!adding && (
          <button type="button" onClick={() => setAdding(true)} className={`${inlineActionClass} max-sm:min-h-11`}>
            Add in your words
          </button>
        )}
      </div>
      {statements.length > 0 && (
        <ul className="border-b border-line-subtle">
          {shown.map(line)}
          {earlier.length > 0 && (
            <li className="border-t border-line-subtle py-2.5">
              <Disclosure label={`Earlier (${earlier.length})`}>
                <ul>{earlier.map(line)}</ul>
              </Disclosure>
            </li>
          )}
        </ul>
      )}
      {adding && (
        <div className={statements.length > 0 ? "mt-4" : "border-t border-line-subtle pt-3.5"}>
          <StatementForm action={action} label="Describe what you're looking for" onCancel={statements.length > 0 ? () => setAdding(false) : undefined} />
        </div>
      )}
    </section>
  );
}

/**
 * What the person wants, in their own words. Narrow's reading comes back
 * in three parts: what it understood, what it isn't sure it read right,
 * and what it couldn't interpret (kept, never dropped).
 */
export function StatementForm({
  action,
  label = "What are you looking for?",
  onCancel,
}: {
  action: (state: StatementState, form: FormData) => Promise<StatementState>;
  label?: string;
  onCancel?: () => void;
}) {
  const [state, formAction, pending] = useActionState(action, {});
  const id = useId();
  const help = useId();
  const result = state.result;
  return (
    <div>
      <form action={formAction}>
        <label htmlFor={id} className={labelClass}>
          {label}
        </label>
        <textarea
          id={id}
          name="statement"
          rows={3}
          required
          maxLength={2000}
          aria-describedby={help}
          defaultValue={state.error ? state.submitted : undefined}
          placeholder="I want small product teams, at least $140k USD, and no pure SRE roles."
          className={textareaClass}
        />
        <div className="mt-3 flex flex-wrap items-center gap-x-4 gap-y-2">
          <Button type="submit" variant="secondary" disabled={pending} loading={pending} className="max-sm:h-11">
            Update preferences
          </Button>
          {onCancel && !result && (
            <Button variant="ghost" onClick={onCancel} disabled={pending} className="max-sm:h-11">
              Cancel
            </Button>
          )}
          <p id={help} className="text-caption text-fg-muted">
            Kept word for word.
          </p>
        </div>
      </form>
      <div aria-live="polite" className="mt-4 space-y-3">
        {state.error && (
          <Notice tone="error" role="alert" title={state.error.title}>
            {state.error.message}
          </Notice>
        )}
        {result && <Interpretation result={result} />}
      </div>
    </div>
  );
}

export function Interpretation({ result }: { result: PreferenceUpdateResult }) {
  const understood = result.interpreted.filter((p) => p.certainty !== "uncertain");
  const heading = "text-[13px] font-semibold text-fg";
  return (
    <div className="space-y-3 border-t border-line-subtle pt-4 text-[14px]">
      {result.unchanged ? (
        <p>Already in effect: nothing changed.</p>
      ) : (
        <>
          {understood.length > 0 && (
            <div>
              <h3 className={heading}>Understood</h3>
              <ul className="mt-1.5 space-y-1">
                {understood.map((p) => (
                  <li key={p.id} className="flex gap-2.5">
                    <span aria-hidden="true" className="mt-[0.55em] size-[5px] shrink-0 rounded-[1px] bg-fg" />
                    <span>
                      <PreferenceLine p={p} />
                    </span>
                  </li>
                ))}
              </ul>
            </div>
          )}
          {result.uncertain.length > 0 && (
            <div>
              <h3 className={heading}>Not sure I read these right</h3>
              <ul className="mt-1.5 space-y-1">
                {result.uncertain.map((p) => (
                  <li key={p.id} className="flex gap-2.5">
                    <span aria-hidden="true" className="mt-[0.55em] size-[5px] shrink-0 rounded-[1px] border border-fg-secondary" />
                    <span>
                      <span className="nr-inferred">
                        <PreferenceLine p={p} />
                      </span>
                      {p.note && <span className="text-fg-secondary"> — {p.note}</span>}
                    </span>
                  </li>
                ))}
              </ul>
            </div>
          )}
          {result.replaced.length > 0 && (
            <p className="text-fg-secondary">
              Replaced: {result.replaced.map((p) => `${STANCE_LABEL[p.stance] ?? p.stance} ${p.value}`).join("; ")}
            </p>
          )}
        </>
      )}
      {result.not_understood.length > 0 && (
        <div>
          <h3 className={heading}>I couldn&apos;t interpret</h3>
          <ul className="mt-1.5 space-y-1">
            {result.not_understood.map((part) => (
              <li key={part} className="flex gap-2.5 text-fg-secondary">
                <span aria-hidden="true" className="mt-[0.55em] size-[5px] shrink-0 rounded-[1px] border border-fg-muted" />
                <q>{part}</q>
              </li>
            ))}
          </ul>
          <p className="mt-1.5 text-caption text-fg-muted">Kept as you wrote it. Try saying these parts another way.</p>
        </div>
      )}
    </div>
  );
}
