"use client";

import { useActionState, useId, useState, useTransition } from "react";

import type { ActionResult, StatementState } from "@/app/actions";
import type { PreferenceInput, PreferenceUpdateResult, PreferenceView } from "@/lib/api-types";
import { STANCE_LABEL } from "@/lib/format";

import { Button, Notice } from "./ui";


function PreferenceLine({ p }: { p: PreferenceView }) {
  return (
    <>
      <span className="font-medium">{STANCE_LABEL[p.stance] ?? p.stance}:</span> {p.value}
    </>
  );
}

/**
 * What the person wants, in their own words. JobHunt's reading comes back
 * in three parts: what it understood, what it isn't sure it read right,
 * and what it couldn't interpret (kept, never dropped).
 */
export function StatementForm({
  action,
}: {
  action: (state: StatementState, form: FormData) => Promise<StatementState>;
}) {
  const [state, formAction, pending] = useActionState(action, {});
  const id = useId();
  const result = state.result;
  return (
    <div>
      <form action={formAction} className="space-y-3">
        <label htmlFor={id} className="block font-medium">
          What are you looking for?
        </label>
        <textarea
          id={id}
          name="statement"
          rows={3}
          required
          maxLength={2000}
          defaultValue={state.error ? state.submitted : undefined}
          placeholder="I want small product teams, at least $140k USD, and no pure SRE roles."
          className="w-full rounded-md border border-line-strong bg-surface px-3 py-2"
        />
        <div className="flex items-center gap-3">
          <Button type="submit" variant="primary" disabled={pending}>
            {pending ? "Reading…" : "Update preferences"}
          </Button>
          <p className="text-sm text-muted">Say it however you like. It&apos;s kept word for word.</p>
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
  return (
    <div className="rounded-xl border border-line bg-surface p-5 text-sm">
      {result.unchanged ? (
        <p>Already in effect: nothing changed.</p>
      ) : (
        <>
          {understood.length > 0 && (
            <div>
              <h3 className="font-semibold">Understood</h3>
              <ul className="mt-1 list-disc space-y-0.5 pl-5">
                {understood.map((p) => (
                  <li key={p.id}>
                    <PreferenceLine p={p} />
                  </li>
                ))}
              </ul>
            </div>
          )}
          {result.uncertain.length > 0 && (
            <div className="mt-3">
              <h3 className="font-semibold">Not sure I read these right</h3>
              <ul className="mt-1 list-disc space-y-0.5 pl-5">
                {result.uncertain.map((p) => (
                  <li key={p.id}>
                    <PreferenceLine p={p} />
                    {p.note && <span className="text-muted"> — {p.note}</span>}
                  </li>
                ))}
              </ul>
            </div>
          )}
          {result.replaced.length > 0 && (
            <p className="mt-3 text-muted">
              Replaced: {result.replaced.map((p) => `${STANCE_LABEL[p.stance] ?? p.stance} ${p.value}`).join("; ")}
            </p>
          )}
        </>
      )}
      {result.not_understood.length > 0 && (
        <div className="mt-3">
          <h3 className="font-semibold">I couldn&apos;t interpret</h3>
          <ul className="mt-1 space-y-0.5">
            {result.not_understood.map((part) => (
              <li key={part}>
                <q>{part}</q>
              </li>
            ))}
          </ul>
          <p className="mt-1 text-muted">Kept as you wrote it. Try saying these parts another way.</p>
        </div>
      )}
    </div>
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
        className="text-sm text-muted underline hover:text-ink disabled:opacity-50"
      >
        {pending ? "Removing…" : "Remove"}
        <span className="sr-only"> {label}</span>
      </button>
      {error && (
        <span role="alert" className="ml-2 text-sm text-negative">
          {error}
        </span>
      )}
    </>
  );
}

type Kind = "role" | "domain" | "company" | "work_style" | "work_mode" | "region" | "timezone" | "location";

const KINDS: { kind: Kind; label: string; placeholder: string }[] = [
  { kind: "role", label: "Role", placeholder: "backend, platform, founding engineer" },
  { kind: "domain", label: "Domain or product", placeholder: "developer tools, fintech" },
  { kind: "company", label: "Company or team", placeholder: "small-team, founder-led, startup" },
  { kind: "work_style", label: "Way of working", placeholder: "ownership, greenfield, async-communication" },
  { kind: "work_mode", label: "Remote, hybrid or on-site", placeholder: "" },
  { kind: "region", label: "Region you can work in", placeholder: "EU, Americas" },
  { kind: "timezone", label: "Time zone", placeholder: "UTC-3, US hours" },
  { kind: "location", label: "Where you live", placeholder: "Lisbon, Portugal" },
];

function toInput(kind: Kind, value: string, stance: string): PreferenceInput {
  const s = stance as "require" | "want" | "accept" | "avoid";
  switch (kind) {
    case "role":
      return { kind, role: value, stance: s };
    case "domain":
      return { kind, domain: value, stance: s };
    case "company":
      return { kind, company: value, stance: s };
    case "work_style":
      return { kind, aspect: value, stance: s };
    case "work_mode":
      return { kind, mode: value as "remote" | "hybrid" | "onsite", stance: s };
    case "region":
      return { kind, region: value, stance: s };
    case "timezone":
      return { kind, zone: value, stance: s };
    case "location":
      return { kind, place: value };
  }
}

/**
 * Precise preferences, when a sentence isn't the easiest way: one value at
 * a time, or pay (a minimum is a hard floor, a target is what you hope for).
 */
export function PreciseForm({
  set,
}: {
  set: (input: PreferenceInput) => Promise<ActionResult<PreferenceUpdateResult>>;
}) {
  const [kind, setKind] = useState<Kind>("role");
  const [pending, startTransition] = useTransition();
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);
  const ids = { kind: useId(), value: useId(), stance: useId(), min: useId(), target: useId(), currency: useId() };
  const current = KINDS.find((k) => k.kind === kind)!;

  const submit = (input: PreferenceInput) =>
    startTransition(async () => {
      const r = await set(input);
      setMessage(
        r.ok
          ? { ok: true, text: r.data.unchanged ? "Already in effect." : "Saved." }
          : { ok: false, text: `${r.title}. ${r.message}` },
      );
    });

  return (
    <div className="grid gap-6 sm:grid-cols-2">
      <form
        className="space-y-2 rounded-xl border border-line bg-surface p-4"
        onSubmit={(e) => {
          e.preventDefault();
          const data = new FormData(e.currentTarget);
          const value = String(data.get("value") ?? "").trim();
          if (!value) return;
          submit(toInput(kind, value, String(data.get("stance") ?? "want")));
        }}
      >
        <h3 className="font-medium">One preference</h3>
        <label htmlFor={ids.kind} className="block text-sm text-muted">
          About
        </label>
        <select
          id={ids.kind}
          value={kind}
          onChange={(e) => setKind(e.target.value as Kind)}
          className="w-full rounded-md border border-line-strong bg-canvas px-2 py-1.5"
        >
          {KINDS.map((k) => (
            <option key={k.kind} value={k.kind}>
              {k.label}
            </option>
          ))}
        </select>
        <label htmlFor={ids.value} className="block text-sm text-muted">
          Value
        </label>
        {kind === "work_mode" ? (
          <select id={ids.value} name="value" className="w-full rounded-md border border-line-strong bg-canvas px-2 py-1.5">
            <option value="remote">Remote</option>
            <option value="hybrid">Hybrid</option>
            <option value="onsite">On-site</option>
          </select>
        ) : (
          <input
            id={ids.value}
            name="value"
            required
            placeholder={current.placeholder}
            className="w-full rounded-md border border-line-strong bg-canvas px-2 py-1.5"
          />
        )}
        {kind !== "location" && (
          <>
            <label htmlFor={ids.stance} className="block text-sm text-muted">
              How much it matters
            </label>
            <select id={ids.stance} name="stance" defaultValue="want" className="w-full rounded-md border border-line-strong bg-canvas px-2 py-1.5">
              <option value="require">Must have</option>
              <option value="want">Want</option>
              <option value="accept">Fine with</option>
              <option value="avoid">Avoid</option>
            </select>
          </>
        )}
        <Button type="submit" disabled={pending}>
          Add
        </Button>
      </form>

      <form
        className="space-y-2 rounded-xl border border-line bg-surface p-4"
        onSubmit={(e) => {
          e.preventDefault();
          const data = new FormData(e.currentTarget);
          const num = (name: string) => {
            const raw = String(data.get(name) ?? "").replace(/[,_\s]/g, "");
            return raw ? Number(raw) : null;
          };
          const minimum = num("minimum");
          const target = num("target");
          if (minimum === null && target === null) return;
          submit({
            kind: "compensation",
            minimum,
            target,
            currency: String(data.get("currency") ?? "USD").trim().toUpperCase(),
            period: "year",
          });
        }}
      >
        <h3 className="font-medium">Pay, per year</h3>
        <label htmlFor={ids.min} className="block text-sm text-muted">
          Minimum <span className="text-xs">(below this, jobs are left out)</span>
        </label>
        <input id={ids.min} name="minimum" inputMode="numeric" placeholder="120000" className="w-full rounded-md border border-line-strong bg-canvas px-2 py-1.5" />
        <label htmlFor={ids.target} className="block text-sm text-muted">
          Target
        </label>
        <input id={ids.target} name="target" inputMode="numeric" placeholder="150000" className="w-full rounded-md border border-line-strong bg-canvas px-2 py-1.5" />
        <label htmlFor={ids.currency} className="block text-sm text-muted">
          Currency
        </label>
        <input id={ids.currency} name="currency" defaultValue="USD" maxLength={3} className="w-24 rounded-md border border-line-strong bg-canvas px-2 py-1.5 uppercase" />
        <div>
          <Button type="submit" disabled={pending}>
            Save pay
          </Button>
        </div>
      </form>
      <p aria-live="polite" className={`text-sm sm:col-span-2 ${message?.ok === false ? "text-negative" : "text-muted"}`}>
        {message?.text}
      </p>
    </div>
  );
}
