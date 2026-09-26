"use client";

import { useId, useState, useTransition } from "react";

import type { ActionResult } from "@/app/actions";
import type { Clarify, PreferenceInput, PreferenceUpdateResult, PreferenceView } from "@/lib/api-types";

import { Button, helpClass, inputClass, labelClass } from "./ui";

export type ClarifyAction = (id: string, set: PreferenceInput[]) => Promise<ActionResult<PreferenceUpdateResult>>;

const CURRENCIES = ["USD", "EUR", "GBP", "BRL", "CAD", "AUD"];

const choiceClass =
  "flex items-center gap-2 text-[14px] text-fg-body max-sm:min-h-11 [&>input]:size-4 [&>input]:accent-fg";

function Choice({ name, value, label, required }: { name: string; value: string; label: string; required?: boolean }) {
  return (
    <label className={choiceClass}>
      <input type="radio" name={name} value={value} required={required} />
      {label}
    </label>
  );
}

function amountText(c: Extract<Clarify, { kind: "pay" }>): string {
  return `${c.amount.toLocaleString("en-US")} per ${c.period}`;
}

/** What a question is about, in a few words. */
export function clarifyTitle(c: Clarify): string {
  return c.kind === "pay" ? `Pay: ${amountText(c)}` : c.value === "small_company" ? "Small companies" : "Small teams";
}

/**
 * A preference Narrow read from someone's words but can't rely on until
 * they answer: whether a pay is a floor or a goal, and in which currency
 * (never assumed); whether a size is a must or a nice-to-have, and whether
 * it's the team or the company. Until then it is unresolved, and says so.
 */
export function ClarifyPreference({ p, clarify }: { p: PreferenceView; clarify: ClarifyAction }) {
  const c = p.clarify;
  const [pending, startTransition] = useTransition();
  const [error, setError] = useState<string | null>(null);
  const ids = { form: useId(), currency: useId(), list: useId() };
  if (!c) return null;

  const submit = (set: PreferenceInput[]) =>
    startTransition(async () => {
      setError(null);
      const r = await clarify(p.id, set);
      if (!r.ok) setError(`${r.title}. ${r.message}`);
    });

  return (
    <form
      aria-labelledby={ids.form}
      className="mt-2 rounded-md border border-line bg-inset p-4 max-sm:p-3.5"
      onSubmit={(e) => {
        e.preventDefault();
        const data = new FormData(e.currentTarget);
        if (c.kind === "pay") {
          const bound = String(data.get("bound"));
          const currency = String(data.get("currency") ?? "").trim().toUpperCase();
          if (!/^[A-Z]{3}$/.test(currency)) {
            setError("Choose the currency: a three-letter code like USD or EUR.");
            return;
          }
          submit([
            {
              kind: "compensation",
              minimum: bound === "minimum" ? c.amount : null,
              target: bound === "target" ? c.amount : null,
              currency,
              period: c.period as "year" | "month" | "day" | "hour",
              applies_to: (c.applies_to as "employment" | "contract" | undefined) ?? null,
            },
          ]);
        } else {
          const scope = String(data.get("scope"));
          const stance = String(data.get("stance")) as "require" | "want";
          const kinds = scope === "both" ? ["small_team", "small_company"] : [scope];
          submit(kinds.map((company) => ({ kind: "company" as const, company, stance })));
        }
      }}
    >
      <p id={ids.form} className="text-[13px] font-semibold text-fg">
        Needs your answer
      </p>
      {c.kind === "pay" ? (
        <>
          <p className="mt-1 text-[13px] text-fg-secondary">
            {c.currency
              ? `Read as ${c.bound === "minimum" ? "a minimum" : "a target"} of ${c.currency} ${amountText(c)}.`
              : `Until you say the currency, ${amountText(c)} isn't compared with any job's pay.`}
          </p>
          <fieldset className="mt-3">
            <legend className={labelClass}>{amountText(c)}: at least, or around?</legend>
            <div className="flex flex-wrap gap-x-5 gap-y-1">
              <Choice name="bound" value="minimum" label="At least (a hard floor)" required />
              <Choice name="bound" value="target" label="Around (what I'm aiming for)" required />
            </div>
          </fieldset>
          <div className="mt-3">
            <label htmlFor={ids.currency} className={labelClass}>
              Currency
            </label>
            <input
              id={ids.currency}
              name="currency"
              list={ids.list}
              required
              maxLength={3}
              defaultValue={c.currency ?? ""}
              placeholder="USD"
              autoComplete="off"
              className={`${inputClass} w-28 uppercase`}
            />
            <datalist id={ids.list}>
              {CURRENCIES.map((code) => (
                <option key={code} value={code} />
              ))}
            </datalist>
          </div>
        </>
      ) : (
        <>
          <p className="mt-1 text-[13px] text-fg-secondary">
            Until you answer, it only nudges the order; it never leaves anything out.
          </p>
          <fieldset className="mt-3">
            <legend className={labelClass}>{c.value === "small_company" ? "Small companies" : "Small teams"}: which size do you mean?</legend>
            <div className="flex flex-wrap gap-x-5 gap-y-1">
              <Choice name="scope" value="small_team" label="The team I'd join" required />
              <Choice name="scope" value="small_company" label="The whole company" required />
              <Choice name="scope" value="both" label="Both" required />
            </div>
          </fieldset>
          <fieldset className="mt-3">
            <legend className={labelClass}>Must have, or nice to have?</legend>
            <div className="flex flex-wrap gap-x-5 gap-y-1">
              <Choice name="stance" value="require" label="Must have" required />
              <Choice name="stance" value="want" label="Nice to have" required />
            </div>
          </fieldset>
          <p className={helpClass}>
            Must have: a posting that says otherwise is left out; one that doesn&apos;t say is still shown, marked unresolved. Nice to have: it
            only changes the order.
          </p>
        </>
      )}
      <div className="mt-3 flex flex-wrap items-center gap-3">
        <Button type="submit" variant="secondary" disabled={pending} loading={pending} className="max-sm:h-11">
          Confirm
        </Button>
        {error && (
          <span role="alert" className="text-[13px] text-danger">
            {error}
          </span>
        )}
      </div>
    </form>
  );
}
