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

type Kind =
  | "role"
  | "compensation"
  | "work_mode"
  | "region"
  | "timezone"
  | "location"
  | "company"
  | "domain"
  | "work_style";

// What a preference can be about: the structured model the API accepts.
const KINDS: { kind: Kind; label: string; placeholder: string }[] = [
  { kind: "role", label: "Role", placeholder: "backend, platform, founding engineer" },
  { kind: "compensation", label: "Pay", placeholder: "120,000" },
  { kind: "work_mode", label: "Remote, hybrid or on-site", placeholder: "" },
  { kind: "region", label: "Region you can work in", placeholder: "EU, Americas" },
  { kind: "timezone", label: "Time zone", placeholder: "UTC-3, US hours" },
  { kind: "location", label: "Where you live", placeholder: "Lisbon, Portugal" },
  { kind: "company", label: "Company or team", placeholder: "small-team, founder-led, startup" },
  { kind: "domain", label: "Domain or product", placeholder: "developer tools, fintech" },
  { kind: "work_style", label: "Way of working", placeholder: "ownership, greenfield, async-communication" },
];

const STANCES = [
  { value: "require", label: "Must have" },
  { value: "want", label: "Want" },
  { value: "accept", label: "Fine with" },
  { value: "avoid", label: "Avoid" },
];

const PAY_RULES = [
  { value: "minimum", label: "At least (a hard floor)" },
  { value: "target", label: "Aiming for" },
];

type Stance = "require" | "want" | "accept" | "avoid";

function toInput(kind: Exclude<Kind, "compensation">, value: string, stance: Stance): PreferenceInput {
  switch (kind) {
    case "role":
      return { kind, role: value, stance };
    case "domain":
      return { kind, domain: value, stance };
    case "company":
      return { kind, company: value, stance };
    case "work_style":
      return { kind, aspect: value, stance };
    case "work_mode":
      return { kind, mode: value as "remote" | "hybrid" | "onsite", stance };
    case "region":
      return { kind, region: value, stance };
    case "timezone":
      return { kind, zone: value, stance };
    case "location":
      return { kind, place: value };
  }
}

/**
 * A precise preference: what it's about, the rule, the value. The same
 * structured model the API, the CLI and AI assistants use; no free text
 * is parsed here. Pay has two rules: a minimum is a hard floor, a target
 * is what you hope for.
 */
export function AddPreference({ set }: { set: (input: PreferenceInput) => Promise<ActionResult<PreferenceUpdateResult>> }) {
  const [kind, setKind] = useState<Kind>("role");
  const [pending, startTransition] = useTransition();
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);
  const ids = { kind: useId(), rule: useId(), value: useId(), currency: useId(), help: useId() };
  const current = KINDS.find((k) => k.kind === kind)!;

  const submit = (input: PreferenceInput) =>
    startTransition(async () => {
      const r = await set(input);
      setMessage(r.ok ? { ok: true, text: r.data.unchanged ? "Already in effect." : "Saved." } : { ok: false, text: `${r.title}. ${r.message}` });
    });

  return (
    <form
      aria-describedby={ids.help}
      className="grid gap-3 rounded-lg border border-line bg-inset p-[18px] max-sm:p-4 md:grid-cols-[200px_minmax(0,1fr)_auto] md:items-end"
      onSubmit={(e) => {
        e.preventDefault();
        setMessage(null);
        const data = new FormData(e.currentTarget);
        const value = String(data.get("value") ?? "").trim();
        const rule = String(data.get("rule") ?? "");
        if (!value) return;
        if (kind === "compensation") {
          const amount = Number(value.replace(/[,_\s]/g, ""));
          if (!Number.isFinite(amount) || amount <= 0) {
            setMessage({ ok: false, text: "Enter the amount as a number, like 120000." });
            return;
          }
          submit({
            kind: "compensation",
            minimum: rule === "minimum" ? amount : null,
            target: rule === "target" ? amount : null,
            currency: String(data.get("currency") ?? "USD").trim().toUpperCase(),
            period: "year",
          });
        } else {
          submit(toInput(kind, value, (rule || "want") as Stance));
        }
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
          {kind === "location" ? (
            <p id={ids.rule} className="flex h-10 items-center text-[14px] text-fg-secondary max-sm:h-11">
              I live in
            </p>
          ) : (
            <select
              key={kind === "compensation" ? "pay" : "stance"}
              id={ids.rule}
              name="rule"
              defaultValue={kind === "compensation" ? "minimum" : "want"}
              className={`${selectClass} h-10 text-[14px]`}
            >
              {(kind === "compensation" ? PAY_RULES : STANCES).map((s) => (
                <option key={s.value} value={s.value}>
                  {s.label}
                </option>
              ))}
            </select>
          )}
        </div>
        <div className="min-w-0">
          <label htmlFor={ids.value} className={labelClass}>
            Value{kind === "compensation" && <span className="text-fg-muted"> (per year)</span>}
          </label>
          {kind === "work_mode" ? (
            <select id={ids.value} name="value" className={`${selectClass} h-10 text-[14px]`}>
              <option value="remote">Remote</option>
              <option value="hybrid">Hybrid</option>
              <option value="onsite">On-site</option>
            </select>
          ) : kind === "compensation" ? (
            <div className="flex gap-2">
              <input id={ids.value} name="value" required inputMode="numeric" placeholder={current.placeholder} className={`${inputClass} h-10 text-[14px]`} />
              <label htmlFor={ids.currency} className="sr-only">
                Currency
              </label>
              <input
                id={ids.currency}
                name="currency"
                defaultValue="USD"
                maxLength={3}
                className={`${inputClass} h-10 w-20 shrink-0 text-[14px] uppercase`}
              />
            </div>
          ) : (
            <input id={ids.value} name="value" required placeholder={current.placeholder} className={`${inputClass} h-10 text-[14px]`} />
          )}
        </div>
      </div>
      <Button type="submit" variant="primary" size="lg" disabled={pending} loading={pending} className="max-sm:h-11 max-sm:w-full">
        Add preference
      </Button>
      <p id={ids.help} className={`${helpClass} md:col-span-3 ${message?.ok === false ? "text-danger" : ""}`} aria-live="polite">
        {message?.text ?? "What you tell Narrow always outranks what it has learned. A pay minimum is a hard floor: below it, roles are left out."}
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
