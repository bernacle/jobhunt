"use client";

import { useActionState, useId, useState, useTransition } from "react";

import type { ActionResult, StatementState } from "@/app/actions";
import type { PreferenceInput, PreferenceUpdateResult, PreferenceView } from "@/lib/api-types";
import { STANCE_LABEL } from "@/lib/format";

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
      className="grid gap-3 rounded-lg border border-line bg-inset p-[18px] max-sm:p-4 md:grid-cols-[200px_minmax(0,1fr)_auto] md:items-end"
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
        <select id={ids.kind} value={kind} onChange={(e) => setKind(e.target.value as Kind)} className={`${selectClass} h-10 text-[14px]`}>
          {KINDS.map((k) => (
            <option key={k.kind} value={k.kind}>
              {k.label}
            </option>
          ))}
        </select>
      </div>
      <div className="grid min-w-0 gap-3 sm:grid-cols-[160px_minmax(0,1fr)]">
        <div className="min-w-0">
          <label htmlFor={ids.rule} className={labelClass}>
            Rule
          </label>
          <select id={ids.rule} name="rule" defaultValue="want" className={`${selectClass} h-10 text-[14px]`}>
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
          <input id={ids.value} name="value" required placeholder={current.placeholder} className={`${inputClass} h-10 text-[14px]`} />
        </div>
      </div>
      <Button type="submit" variant="primary" size="lg" disabled={pending} loading={pending} className="max-sm:h-11 max-sm:w-full">
        Add preference
      </Button>
      <p id={ids.help} className={`${helpClass} md:col-span-3 ${message?.ok === false ? "text-danger" : ""}`} aria-live="polite">
        {message?.text ?? "What you tell Narrow always outranks what it has learned. Must have is a requirement; the others only change the order."}
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
 * What the person wants, in their own words. Narrow's reading comes back
 * in three parts: what it understood, what it isn't sure it read right,
 * and what it couldn't interpret (kept, never dropped).
 */
export function StatementForm({
  action,
  label = "What are you looking for?",
}: {
  action: (state: StatementState, form: FormData) => Promise<StatementState>;
  label?: string;
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
          <p id={help} className="text-caption text-fg-muted">
            Say it however you like. It&apos;s kept word for word.
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
